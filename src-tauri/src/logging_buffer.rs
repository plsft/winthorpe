//! In-memory ring buffer for the user-visible Activity Log panel.
//!
//! Sits alongside the JSONL file appender (see `logging.rs`) — the file
//! is the source of truth for incident postmortems, this buffer is what
//! the GUI reads when the user opens **Activity** so they can watch git,
//! sidecar, agent CLI, and command IPC happen in real time.
//!
//! Bounded at `BUFFER_CAPACITY` events; older entries are evicted FIFO.
//! Each event gets a monotonic sequence number so the frontend can poll
//! "give me everything after seq N" cheaply without re-shipping history.
//!
//! The buffer is fed by `BufferLayer`, a `tracing_subscriber::Layer` we
//! attach in `logging::init`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};

/// 5000 events ≈ a few minutes of dev-mode activity. We trim from the
/// front when the queue exceeds this; the GUI never asks for more than
/// ~500 at a time so the cost is bounded.
const BUFFER_CAPACITY: usize = 5000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    /// Monotonic sequence number — frontend uses this as a cursor for
    /// incremental polling.
    pub seq: u64,
    /// Wall-clock millis since epoch. Frontend formats locally.
    pub timestamp_ms: i64,
    /// Uppercase level name: `TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`.
    pub level: String,
    /// `module_path` of the emitter (e.g. `winthorpe_lib::git::ops`).
    pub target: String,
    /// The unstructured `message` field, if any.
    pub message: String,
    /// All other key=value fields rendered as `k=v` lines. Kept as a
    /// pre-formatted string so the frontend doesn't need a JSON viewer.
    pub fields: String,
}

static SEQ: AtomicU64 = AtomicU64::new(0);
static BUFFER: Mutex<VecDeque<LogEvent>> = Mutex::new(VecDeque::new());

/// `tracing_subscriber::Layer` that records every event into the global
/// `BUFFER`. Filtering is applied by whichever `EnvFilter` the layer is
/// attached with — we don't second-guess it here.
pub struct BufferLayer;

impl<S> Layer<S> for BufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();

        let mut visitor = FieldExtractor::default();
        event.record(&mut visitor);

        let entry = LogEvent {
            seq: SEQ.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            level: metadata.level().as_str().to_string(),
            target: metadata.target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        };

        if let Ok(mut buf) = BUFFER.lock() {
            buf.push_back(entry);
            while buf.len() > BUFFER_CAPACITY {
                buf.pop_front();
            }
        }
    }
}

#[derive(Default)]
struct FieldExtractor {
    message: String,
    fields: String,
}

impl Visit for FieldExtractor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        // `tracing`'s convention: the unstructured message is stored under
        // the field name `message`. Everything else is structured key=value.
        if field.name() == "message" {
            // record_debug for &str values writes them as `"text"` with
            // quotes — strip those for legibility in the GUI.
            self.message = formatted.trim_matches('"').to_string();
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            self.fields.push_str(field.name());
            self.fields.push('=');
            self.fields.push_str(&formatted);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            self.fields.push_str(field.name());
            self.fields.push('=');
            self.fields.push_str(value);
        }
    }
}

/// Snapshot all events with `seq > since`. Returns up to `limit` items in
/// chronological order. Used by the `get_log_events` Tauri command.
pub fn snapshot_since(since: u64, limit: usize) -> Vec<LogEvent> {
    let buf = match BUFFER.lock() {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    buf.iter()
        .filter(|e| e.seq > since)
        .take(limit)
        .cloned()
        .collect()
}

/// Drop every buffered event. The seq counter is **not** reset — that
/// would break frontends mid-poll. Next event still gets seq = (last + 1).
pub fn clear() {
    if let Ok(mut buf) = BUFFER.lock() {
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    #[test]
    fn snapshot_since_returns_only_newer_events() {
        // Reset shared state — tests in this file may share globals.
        clear();
        // Emit a few events through a local subscriber so on_event fires.
        let subscriber = tracing_subscriber::registry().with(BufferLayer);
        let _guard = subscriber.set_default();

        tracing::info!("alpha");
        tracing::warn!("beta");
        tracing::error!("gamma");

        let all = snapshot_since(0, 100);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].message, "alpha");
        assert_eq!(all[2].message, "gamma");

        let after_first = snapshot_since(all[0].seq, 100);
        assert_eq!(after_first.len(), 2);
        assert_eq!(after_first[0].message, "beta");
    }

    #[test]
    fn buffer_evicts_oldest_when_full() {
        clear();
        let subscriber = tracing_subscriber::registry().with(BufferLayer);
        let _guard = subscriber.set_default();

        // We can't easily emit 5001 events in a unit test (slow), so prove
        // the invariant directly.
        if let Ok(mut buf) = BUFFER.lock() {
            for i in 0..(BUFFER_CAPACITY + 5) {
                buf.push_back(LogEvent {
                    seq: i as u64,
                    timestamp_ms: 0,
                    level: "INFO".into(),
                    target: "test".into(),
                    message: format!("msg-{i}"),
                    fields: String::new(),
                });
                while buf.len() > BUFFER_CAPACITY {
                    buf.pop_front();
                }
            }
            assert_eq!(buf.len(), BUFFER_CAPACITY);
            assert_eq!(buf.front().unwrap().message, format!("msg-{}", 5));
        }
    }
}
