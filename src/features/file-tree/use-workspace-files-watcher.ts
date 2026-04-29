import { useQueryClient } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import {
	startWorkspaceFilesWatcher,
	stopWorkspaceFilesWatcher,
	type WorkspaceFilesChangedPayload,
} from "@/lib/api";

const EVENT_NAME = "workspace-files-changed";

/**
 * Subscribe to filesystem events for the active workspace.
 *
 * Lifecycle:
 *   - On (workspaceId, root) change: tell the Rust backend to watch the
 *     new directory (idempotent — backend dedupes same id+root). Stops
 *     the prior workspace's watcher first.
 *   - On any `workspace-files-changed` event for the active workspace:
 *     - Invalidate the React Query for `workspaceTree` so the FileTree
 *       reloads its directory listing.
 *     - Call `onPathsChanged(paths)` so callers can react to specific
 *       file changes (e.g. reload the matching open editor tabs).
 *   - On unmount or workspace switch: stop the watcher to release the
 *     OS file-system handle.
 *
 * Pass `null` workspaceId to suspend (no watcher started, no events
 * subscribed).
 */
export function useWorkspaceFilesWatcher(
	workspaceId: string | null,
	workspaceRootPath: string | null,
	onPathsChanged?: (paths: string[]) => void,
): void {
	const queryClient = useQueryClient();
	// Hold the latest callback in a ref so re-renders don't tear down
	// + rebuild the listen subscription. Only workspaceId / root changes
	// should reset the watcher.
	const callbackRef = useRef(onPathsChanged);
	callbackRef.current = onPathsChanged;

	useEffect(() => {
		if (!workspaceId || !workspaceRootPath) {
			return;
		}

		let cancelled = false;
		let unlisten: UnlistenFn | null = null;

		void (async () => {
			try {
				await startWorkspaceFilesWatcher(workspaceId, workspaceRootPath);
			} catch (error) {
				// Watcher failure is non-fatal — the tree will just not
				// auto-refresh. Log and carry on.
				console.warn("[file-watcher] start failed:", error);
			}
			if (cancelled) return;

			try {
				unlisten = await listen<WorkspaceFilesChangedPayload>(
					EVENT_NAME,
					(event) => {
						const payload = event.payload;
						// Filter: only react to events for the active workspace.
						// (The backend tags events; this guard is defensive
						// against listener leakage during fast workspace switches.)
						if (payload.workspaceId !== workspaceId) return;
						queryClient.invalidateQueries({
							queryKey: ["workspaceTree", workspaceRootPath],
						});
						callbackRef.current?.(payload.paths);
					},
				);
			} catch (error) {
				console.warn("[file-watcher] listen failed:", error);
			}
		})();

		return () => {
			cancelled = true;
			if (unlisten) {
				unlisten();
				unlisten = null;
			}
			// Best-effort stop — fire-and-forget; the watcher will get
			// dropped on app exit anyway if this fails.
			void stopWorkspaceFilesWatcher(workspaceId).catch((error) => {
				console.warn("[file-watcher] stop failed:", error);
			});
		};
	}, [workspaceId, workspaceRootPath, queryClient]);
}
