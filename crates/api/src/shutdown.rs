//! The api's shutdown signal (graceful shutdown, 2026-10-08).
//!
//! Before this the api installed no SIGTERM handler, so a pod being
//! replaced was killed outright: every in-flight request (a producer's
//! `/private` POST included) was reset, and the background loops' advisory
//! locks were only freed once Postgres noticed the dead socket. Plan 1B.10's
//! exit criterion, "an api deploy with zero failed requests", needs the
//! opposite.
//!
//! [`ShutdownSignal`] is the one flag every part of the shutdown watches:
//! the listener ([`crate::edge::serve_with_shutdown`]) stops accepting and
//! drains, `/public/ready` ([`crate::readiness`]) answers 503, and the
//! background loops stop and release their locks. [`wait_for_os_signal`]
//! is what triggers it in the binary; tests trigger it by hand.

use std::sync::Arc;

use tokio::sync::watch;

/// A one-way "shutting down" flag, cheap to clone and to check.
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    tx: Arc<watch::Sender<bool>>,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal {
    pub fn new() -> Self {
        Self {
            tx: Arc::new(watch::Sender::new(false)),
        }
    }

    /// Starts the shutdown. Idempotent.
    pub fn trigger(&self) {
        self.tx.send_replace(true);
    }

    /// Whether [`Self::trigger`] has been called.
    pub fn is_triggered(&self) -> bool {
        *self.tx.borrow()
    }

    /// Resolves once [`Self::trigger`] has been called (at once if it
    /// already has). The future owns its receiver, so it is `'static`.
    pub fn triggered(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.tx.subscribe();
        async move {
            // The sender lives as long as any clone of `self`; if every
            // clone is gone nobody can trigger it, and an `Err` here means
            // exactly that -- treat it as triggered rather than hang.
            let _ = rx.wait_for(|triggered| *triggered).await;
        }
    }
}

/// Resolves on SIGTERM (what the kubelet sends, after the `preStop` hook)
/// or Ctrl-C. The same handler the ingest-writer installs. Call it only
/// once the server is about to listen: before that SIGTERM keeps its
/// default action (exit at once), so a pod stopped mid-migration still
/// stops promptly.
pub async fn wait_for_os_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = sigterm.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(err) => {
                tracing::warn!(error = ?err, "could not install a SIGTERM handler; Ctrl-C only");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn triggered_resolves_only_after_trigger() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_triggered());
        let waiting = tokio::spawn(signal.triggered());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        signal.clone().trigger();
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("resolves once triggered")
            .expect("task");
        assert!(signal.is_triggered());
        // Already triggered: resolves at once.
        tokio::time::timeout(Duration::from_secs(1), signal.triggered())
            .await
            .expect("resolves at once");
    }
}
