//! Cross-platform process supervision.
//!
//! ## What this provides
//!
//! - `ProcessGroup` — RAII handle that lifes-extends-controls a parent process
//!   and every descendant it spawns. Drop ⇒ entire tree dies.
//! - `assign_to_group` / `kill_group` — lower-level helpers used by callers
//!   that already own a `Child` and want to assign it after spawn.
//!
//! ## Implementation
//!
//! - **Unix:** delegates to `libc::setpgid` + `killpg`. Negative-PID `kill`
//!   targets the entire group atomically.
//! - **Windows:** wraps a Win32 Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//!   The kernel guarantees that closing the last handle to the job kills every
//!   process inside, so RAII drop semantics give us the same "kill the whole
//!   tree" property as `killpg` without explicit traversal.
//!
//! ## Why not just use Phase 1's `taskkill /T /F`?
//!
//! `taskkill` requires walking the snapshot of running PIDs and racing against
//! reparenting; if Bun fork-exec's a child during the kill window, the new PID
//! escapes. Job Objects are race-free: the kernel knows about every process
//! attached to the job, including those spawned mid-kill.

#[cfg(unix)]
mod imp {
    use anyhow::Result;
    use std::process::Command;

    /// Configure `cmd` to spawn its child in a fresh process group whose PGID
    /// equals the child's PID. Subsequent `kill_group(child.id())` then signals
    /// the entire descendant tree.
    ///
    /// Caller must invoke this **before** `cmd.spawn()`.
    pub fn isolate_process_group(cmd: &mut Command) {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    /// Send SIGTERM (cooperative) or SIGKILL (force) to the entire process
    /// group rooted at `pid`.
    pub fn kill_group(pid: u32, force: bool) -> Result<()> {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // Negative PID targets the group. ESRCH on a dead group is harmless.
        unsafe {
            libc::kill(-(pid as libc::pid_t), signal);
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result};
    use std::process::{Child, Command};
    use std::sync::Arc;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::OpenProcess;
    use windows::Win32::System::Threading::PROCESS_ALL_ACCESS;

    /// RAII wrapper around a Win32 Job Object. Closing the last handle kills
    /// every process attached to the job (we set KILL_ON_JOB_CLOSE).
    ///
    /// Cloneable: clones share the same job (Arc'd handle). The job dies when
    /// the last clone is dropped *and* no other handle to the underlying job
    /// kernel object remains.
    #[derive(Clone)]
    pub struct JobObject {
        handle: Arc<JobHandle>,
    }

    struct JobHandle(HANDLE);

    // SAFETY: HANDLE is just a kernel-object pointer; it's safe to send/share
    // across threads. The Win32 docs explicitly allow concurrent access to job
    // object handles.
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // Explicitly terminate first — CloseHandle alone is enough because
            // KILL_ON_JOB_CLOSE is set, but TerminateJobObject is the
            // documented "kill now" call and avoids any kernel-side delay.
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
                let _ = CloseHandle(self.0);
            }
        }
    }

    impl JobObject {
        /// Create a fresh job object with KILL_ON_JOB_CLOSE.
        pub fn new() -> Result<Self> {
            // SAFETY: passing None for both args creates an unnamed job with
            // default security; failure is reported as an Err HANDLE.
            let handle =
                unsafe { CreateJobObjectW(None, None) }.context("CreateJobObjectW failed")?;

            // Set the kill-on-close limit. The cast/zeroed pattern is the
            // canonical way to populate JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    ..Default::default()
                },
                ..Default::default()
            };

            unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .context("SetInformationJobObject(KILL_ON_JOB_CLOSE) failed")?;
            }

            // Suppress unused warning — we only set it, never read it back.
            let _ = &mut info;

            Ok(Self {
                handle: Arc::new(JobHandle(handle)),
            })
        }

        /// Attach `child` to this job. All future descendants of `child`
        /// (granchildren, etc.) inherit the job assignment automatically —
        /// that's the kernel's job-tree semantics.
        ///
        /// Must be called after `child` is spawned. Bun and other runtimes
        /// may briefly run before this assignment lands; callers that need
        /// strict isolation should spawn with `CREATE_SUSPENDED`, assign,
        /// then `ResumeThread` — but for our use case (sidecar startup,
        /// git invocations) the race window is benign (we don't expect the
        /// child to fork before our assign call returns).
        pub fn assign(&self, child: &Child) -> Result<()> {
            let pid = child.id();
            // OpenProcess with PROCESS_ALL_ACCESS gives us the handle the job
            // assignment needs. Phase 7 may narrow the access mask once we
            // know exactly what we need.
            let process_handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid) }
                .with_context(|| format!("OpenProcess failed for PID {pid}"))?;

            let result = unsafe { AssignProcessToJobObject(self.handle.0, process_handle) };

            // Always close the process handle we just opened — the job
            // tracks the process by its kernel object, not by this handle.
            unsafe {
                let _ = CloseHandle(process_handle);
            }

            result.with_context(|| format!("AssignProcessToJobObject failed for PID {pid}"))?;
            Ok(())
        }

        /// Force-kill every process in the job immediately. Equivalent to
        /// dropping the last clone, but explicit when you want to short-circuit
        /// the descendants without waiting for the next teardown step.
        pub fn terminate(&self) {
            unsafe {
                let _ = TerminateJobObject(self.handle.0, 1);
            }
        }
    }

    /// No-op on Windows — process group isolation happens at job-attach time
    /// (after spawn), not at command configuration time. Callers that want
    /// signal isolation (Ctrl+Break) should still set CREATE_NEW_PROCESS_GROUP
    /// via `std::os::windows::process::CommandExt` directly.
    pub fn isolate_process_group(_cmd: &mut Command) {}

    /// Cooperative or forced kill of the process tree rooted at `pid`.
    ///
    /// This is the fallback for callers that haven't migrated to JobObject
    /// yet (Phase 2 incremental rollout). It uses `taskkill` with `/T` to
    /// reach descendants. Prefer `JobObject` for any new code.
    pub fn kill_group(pid: u32, force: bool) -> Result<()> {
        use std::process::Stdio;
        let mut args = vec!["/PID".to_string(), pid.to_string(), "/T".to_string()];
        if force {
            args.push("/F".to_string());
        }
        let _ = Command::new("taskkill")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::JobObject;
