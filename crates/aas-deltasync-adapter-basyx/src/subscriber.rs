//! `BaSyx` MQTT subscriber for event ingestion.

use crate::events::{BasyxEvent, EventParseError};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::time::Duration;
use tokio::sync::mpsc;
use url::Url;

/// Configuration for the `BaSyx` subscriber.
#[derive(Debug, Clone)]
pub struct BasyxSubscriberConfig {
    /// MQTT broker URL (e.g., <tcp://localhost:1883>)
    pub mqtt_broker: String,
    /// Client ID for MQTT connection
    pub client_id: String,
    /// Repository ID to subscribe to
    pub repo_id: String,
    /// Keep-alive interval
    pub keep_alive: Duration,
}

impl Default for BasyxSubscriberConfig {
    fn default() -> Self {
        Self {
            mqtt_broker: "tcp://localhost:1883".to_string(),
            client_id: "aas-deltasync-basyx".to_string(),
            repo_id: "sm-repo".to_string(),
            keep_alive: Duration::from_secs(30),
        }
    }
}

/// MQTT subscriber for `BaSyx` events.
pub struct BasyxSubscriber {
    client: AsyncClient,
    eventloop: EventLoop,
    config: BasyxSubscriberConfig,
}

impl BasyxSubscriber {
    /// Create a new `BaSyx` subscriber.
    ///
    /// # Errors
    ///
    /// Returns error if MQTT connection fails.
    pub fn new(config: BasyxSubscriberConfig) -> Result<Self, SubscriberError> {
        // Parse broker URL
        let (host, port) = parse_mqtt_url(&config.mqtt_broker)?;

        let mut mqtt_options = MqttOptions::new(&config.client_id, host, port);
        mqtt_options.set_keep_alive(config.keep_alive);

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok(Self {
            client,
            eventloop,
            config,
        })
    }

    /// Subscribe to submodel repository events.
    ///
    /// # Errors
    ///
    /// Returns error if subscription fails.
    pub async fn subscribe(&self) -> Result<(), SubscriberError> {
        // Subscribe to all submodel element events for the repository
        let topic = format!("sm-repository/{}/#", self.config.repo_id);

        tracing::info!(topic, "Subscribing to BaSyx events");

        self.client
            .subscribe(&topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| SubscriberError::Subscribe(e.to_string()))?;

        Ok(())
    }

    /// Start receiving events.
    ///
    /// Returns a channel receiver for parsed events.
    pub fn start(mut self) -> mpsc::Receiver<Result<BasyxEvent, EventParseError>> {
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            loop {
                match self.eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let topic = publish.topic.clone();
                        let payload = publish.payload.to_vec();
                        let payload_len = payload.len();

                        tracing::debug!(topic, payload_len, "Received MQTT message");

                        let event = BasyxEvent::parse(&topic, &payload);
                        match &event {
                            Ok(parsed) => {
                                let (id_short_path, has_value) =
                                    parsed.element.as_ref().map_or((None, false), |element| {
                                        (
                                            Some(element.id_short_path.as_str()),
                                            element.value.is_some(),
                                        )
                                    });

                                tracing::debug!(
                                    repo_id = %parsed.repo_id,
                                    submodel_id = %parsed.submodel_id,
                                    event_type = ?parsed.event_type,
                                    id_short_path = ?id_short_path,
                                    has_value,
                                    "Parsed BaSyx event"
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    topic = %topic,
                                    payload_len,
                                    "Failed to parse BaSyx event"
                                );
                            }
                        }

                        if tx.send(event).await.is_err() {
                            tracing::warn!("Event receiver dropped, stopping subscriber");
                            break;
                        }
                    }
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        tracing::info!("Connected to MQTT broker");
                    }
                    Ok(Event::Incoming(Packet::SubAck(_))) => {
                        tracing::info!("Subscription acknowledged");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "MQTT error");
                        // Try to reconnect after a delay
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        rx
    }
}

/// Parse MQTT URL into host and port.
fn parse_mqtt_url(input: &str) -> Result<(String, u16), SubscriberError> {
    if input.contains("://") {
        let url =
            Url::parse(input).map_err(|e| SubscriberError::InvalidUrl(format!("{input}: {e}")))?;

        match url.scheme() {
            "tcp" | "mqtt" => {}
            scheme => {
                return Err(SubscriberError::InvalidUrl(format!(
                    "{input}: unsupported scheme '{scheme}'"
                )));
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| SubscriberError::InvalidUrl(format!("{input}: missing host")))?;
        let port = url.port().unwrap_or(1883);

        return Ok((host.to_string(), port));
    }

    let mut parts = input.split(':');
    let host = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SubscriberError::InvalidUrl(format!("{input}: missing host")))?;
    let port = match parts.next() {
        None => 1883,
        Some(port) => port
            .parse()
            .map_err(|_| SubscriberError::InvalidUrl(format!("{input}: invalid port '{port}'")))?,
    };
    if parts.next().is_some() {
        return Err(SubscriberError::InvalidUrl(format!(
            "{input}: too many ':' separators"
        )));
    }

    Ok((host.to_string(), port))
}

/// Errors that can occur with the subscriber.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SubscriberError {
    /// Invalid MQTT URL
    #[error("invalid MQTT URL: {0}")]
    InvalidUrl(String),
    /// Subscription failed
    #[error("subscription error: {0}")]
    Subscribe(String),
    /// Connection error
    #[error("connection error: {0}")]
    Connection(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mqtt_url_tcp() {
        let (host, port) = parse_mqtt_url("tcp://localhost:1883").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 1883);
    }

    #[test]
    fn parse_mqtt_url_default_port() {
        let (host, port) = parse_mqtt_url("tcp://broker.example.com").unwrap();
        assert_eq!(host, "broker.example.com");
        assert_eq!(port, 1883);
    }

    #[test]
    fn parse_mqtt_url_no_scheme() {
        let (host, port) = parse_mqtt_url("localhost:1883").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 1883);
    }
}
