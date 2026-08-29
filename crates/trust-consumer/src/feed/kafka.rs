//! Production `MovementFeed`: wraps `rdkafka`'s `StreamConsumer` against
//! RDM's Kafka Train Movements product. SASL_SSL is assumed (RDM's Kafka
//! products are described as SASL-authenticated in the design doc's
//! research; the exact mechanism is a startup-time GAP, see `config.rs`).

use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;

use super::MovementFeed;
use crate::config::Config;
use crate::health::ConnectionState;

pub struct KafkaMovementFeed {
    consumer: StreamConsumer,
    connection_state: ConnectionState,
}

impl KafkaMovementFeed {
    pub fn connect(config: &Config, connection_state: ConnectionState) -> anyhow::Result<Self> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_brokers)
            .set("group.id", &config.kafka_consumer_group)
            .set("security.protocol", "SASL_SSL")
            .set("sasl.mechanisms", &config.kafka_sasl_mechanism)
            .set("sasl.username", &config.kafka_sasl_username)
            .set("sasl.password", &config.kafka_sasl_password)
            .set("enable.auto.commit", "false") // explicit commit, see MovementFeed::commit
            .create()?;

        consumer.subscribe(&[&config.kafka_topic])?;

        Ok(Self { consumer, connection_state })
    }
}

#[async_trait]
impl MovementFeed for KafkaMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        match self.consumer.recv().await {
            Ok(message) => {
                self.connection_state.store(true, std::sync::atomic::Ordering::Relaxed);
                let payload = message.payload().ok_or_else(|| anyhow::anyhow!("empty Kafka message payload"))?;
                Ok(vec![String::from_utf8_lossy(payload).into_owned()])
            }
            Err(err) => {
                self.connection_state.store(false, std::sync::atomic::Ordering::Relaxed);
                Err(err.into())
            }
        }
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.consumer.commit_consumer_state(rdkafka::consumer::CommitMode::Async)?;
        Ok(())
    }
}
