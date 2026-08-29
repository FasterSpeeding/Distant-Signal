//! `MovementFeed`: the one trait that stands between the Kafka-specific
//! consumer (`kafka.rs`) and everything else in this crate. See Task 9's
//! doc comment in the implementation plan for why this replaces
//! `wiremock`-style testing here.

pub mod kafka;

use async_trait::async_trait;

#[async_trait]
pub trait MovementFeed: Send {
    /// Returns the next batch of raw JSON message-batch bodies (each
    /// element is one TRUST batch -- itself a JSON array of envelopes, per
    /// `schema::parse_batch`'s input shape) not yet committed. An empty
    /// `Vec` means "nothing new right now," not an error.
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>>;

    /// Commits offsets for everything returned by the most recent
    /// `next_batch` call. Only called after every message in that batch
    /// has been successfully written through to `api` -- see Task 14's
    /// at-least-once framing: a crash between `next_batch` and `commit`
    /// means the same batch is redelivered next time, which the dedup_key
    /// path (Task 4/13) makes safe.
    async fn commit(&mut self) -> anyhow::Result<()>;
}

#[cfg(test)]
pub struct FakeMovementFeed {
    batches: std::collections::VecDeque<Vec<String>>,
    pub committed_count: usize,
}

#[cfg(test)]
impl FakeMovementFeed {
    pub fn new(batches: Vec<Vec<String>>) -> Self {
        Self { batches: batches.into(), committed_count: 0 }
    }
}

#[cfg(test)]
#[async_trait]
impl MovementFeed for FakeMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        Ok(self.batches.pop_front().unwrap_or_default())
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.committed_count += 1;
        Ok(())
    }
}
