//! Typed event streams from Core to the UI.
//!
//! A producer task holds a [`Sink<T>`]; the consumer (UI) holds the matching
//! [`Subscription<T>`]. Dropping the `Subscription` cancels the producer's
//! [`CancellationToken`], so the backend task(s) stop. The receiver is a
//! `tokio::sync::mpsc::UnboundedReceiver`, whose `recv()` future only needs a
//! waker, so it can be awaited from any executor (GPUI, `futures::executor`, …).

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// The receiving side of a backend stream. Dropping it cancels the producer.
pub struct Subscription<T> {
    /// Monotonically increasing id, unique per process. Lets a consumer
    /// discard events from a superseded stream.
    pub id: u64,
    rx: mpsc::UnboundedReceiver<T>,
    token: CancellationToken,
    _guard: DropGuard,
}

impl<T> Subscription<T> {
    /// Wait for the next item. Returns `None` once every producer has finished
    /// (stream ended, cancelled, or context switched).
    pub async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await
    }

    /// Non-blocking receive.
    pub fn try_recv(&mut self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// Drain up to `max` immediately available items (for per-frame coalescing).
    pub fn try_recv_batch(&mut self, max: usize) -> Vec<T> {
        let mut out = Vec::new();
        while out.len() < max {
            match self.rx.try_recv() {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    }

    /// Cancel the producer explicitly (same as dropping the subscription,
    /// but keeps already-buffered items readable).
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// True once the producer has been cancelled (by drop, `cancel`, context
    /// switch, or shutdown).
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

impl<T> Stream for Subscription<T> {
    type Item = T;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.get_mut().rx.poll_recv(cx)
    }
}

impl<T> std::fmt::Debug for Subscription<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .field("cancelled", &self.token.is_cancelled())
            .finish()
    }
}

/// Returned by [`Sink::send`] once the consumer is gone or the stream was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

impl std::fmt::Display for SinkClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("subscription closed")
    }
}

impl std::error::Error for SinkClosed {}

/// The producing side of a [`Subscription`].
pub struct Sink<T> {
    tx: mpsc::UnboundedSender<T>,
    token: CancellationToken,
}

impl<T> Clone for Sink<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            token: self.token.clone(),
        }
    }
}

impl<T> Sink<T> {
    pub fn send(&self, value: T) -> Result<(), SinkClosed> {
        if self.token.is_cancelled() {
            return Err(SinkClosed);
        }
        self.tx.send(value).map_err(|_| SinkClosed)
    }

    /// Send `value`. The event name is informational only (call sites keep a
    /// descriptive channel name); it is not used for routing.
    pub fn emit(&self, _event: &str, value: &T) -> Result<(), SinkClosed>
    where
        T: Clone,
    {
        self.send(value.clone())
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    pub fn is_closed(&self) -> bool {
        self.token.is_cancelled() || self.tx.is_closed()
    }

    /// Resolves when the consumer dropped the subscription or the stream was cancelled.
    pub async fn closed(&self) {
        tokio::select! {
            _ = self.token.cancelled() => {}
            _ = self.tx.closed() => {}
        }
    }
}

/// Create a connected sink/subscription pair whose cancellation token is a
/// child of `parent` (so cancelling `parent` ends the stream too).
pub fn channel<T>(parent: &CancellationToken) -> (Sink<T>, Subscription<T>) {
    let token = parent.child_token();
    let (tx, rx) = mpsc::unbounded_channel();
    let sub = Subscription {
        id: next_id(),
        rx,
        token: token.clone(),
        _guard: token.clone().drop_guard(),
    };
    (Sink { tx, token }, sub)
}

// ── Payload types (stable JSON shape) ───────────────────────────────────

/// `pod-logs://{ns}/{pod}` payload: `{logs, append}` | `{logs}` | `{error}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodLogPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PodLogPayload {
    pub fn line(logs: String) -> Self {
        Self {
            logs: Some(logs),
            append: Some(true),
            error: None,
        }
    }
    pub fn info(logs: String) -> Self {
        Self {
            logs: Some(logs),
            append: None,
            error: None,
        }
    }
    pub fn error(error: String) -> Self {
        Self {
            logs: None,
            append: None,
            error: Some(error),
        }
    }
}

/// `multi-pod-logs://{ns}/{selector}` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiPodLogLine {
    pub pod: String,
    pub color: String,
    pub line: String,
}

/// One pod of a multi-pod log stream and its assigned color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodColor {
    pub name: String,
    pub color: String,
}

/// Result of `stream_multi_pod_logs`: the pod/color list plus the
/// line stream.
#[derive(Debug)]
pub struct MultiPodLogStream {
    pub pods: Vec<PodColor>,
    pub lines: Subscription<MultiPodLogLine>,
}

/// `exec-exit://{session}` payload: `{reason: "completed"}` | `{error}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecExit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ExecExit {
    pub fn completed() -> Self {
        Self {
            reason: Some("completed".into()),
            error: None,
        }
    }
    pub fn error(e: impl Into<String>) -> Self {
        Self {
            reason: None,
            error: Some(e.into()),
        }
    }
}

/// A running exec session. `stdout` carries raw terminal bytes;
/// `exit` resolves once with the exit payload.
/// Dropping `stdout` (or the whole struct) ends the session.
#[derive(Debug)]
pub struct ExecSession {
    pub session_id: String,
    pub stdout: Subscription<Vec<u8>>,
    pub exit: futures::channel::oneshot::Receiver<ExecExit>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_cancels_producer() {
        let root = CancellationToken::new();
        let (sink, sub) = channel::<u32>(&root);
        assert!(sink.send(1).is_ok());
        drop(sub);
        assert!(sink.token().is_cancelled());
        assert_eq!(sink.send(2), Err(SinkClosed));
    }

    #[test]
    fn parent_cancel_ends_stream() {
        let root = CancellationToken::new();
        let (sink, mut sub) = channel::<u32>(&root);
        sink.send(7).unwrap();
        root.cancel();
        assert!(sub.is_cancelled());
        assert_eq!(sub.try_recv(), Some(7));
        drop(sink);
        assert_eq!(futures::executor::block_on(sub.recv()), None);
    }

    #[test]
    fn payload_shapes_are_stable() {
        assert_eq!(
            serde_json::to_value(PodLogPayload::line("a\n".into())).unwrap(),
            serde_json::json!({"logs": "a\n", "append": true})
        );
        assert_eq!(
            serde_json::to_value(PodLogPayload::error("x".into())).unwrap(),
            serde_json::json!({"error": "x"})
        );
        assert_eq!(
            serde_json::to_value(ExecExit::completed()).unwrap(),
            serde_json::json!({"reason": "completed"})
        );
    }

    #[test]
    fn ids_are_unique() {
        let root = CancellationToken::new();
        let (_a, s1) = channel::<()>(&root);
        let (_b, s2) = channel::<()>(&root);
        assert_ne!(s1.id, s2.id);
    }
}
