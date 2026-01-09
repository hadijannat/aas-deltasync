use aas_deltasync_core::{Delta, Hlc};
use aas_deltasync_proto::{DocDelta, TopicScheme};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;
use uuid::Uuid;

fn hash_doc_id(doc_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    doc_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn parse_mqtt_url(url: &str) -> (String, u16) {
    let url = url
        .strip_prefix("tcp://")
        .or_else(|| url.strip_prefix("mqtt://"))
        .unwrap_or(url);

    let parts: Vec<&str> = url.split(':').collect();

    let host = parts.first().copied().unwrap_or("localhost").to_string();
    let port = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1883);

    (host, port)
}

async fn spawn_eventloop(mut eventloop: EventLoop) {
    loop {
        if eventloop.poll().await.is_err() {
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mqtt_delta_roundtrip() {
    if std::env::var("DELTASYNC_INTEGRATION").is_err() {
        eprintln!("Skipping integration test; set DELTASYNC_INTEGRATION=1 to run");
        return;
    }

    let broker = std::env::var("DELTASYNC_MQTT_BROKER")
        .unwrap_or_else(|_| "tcp://localhost:1883".to_string());
    let (host, port) = parse_mqtt_url(&broker);

    let tenant = "integration";
    let scheme = TopicScheme::new(tenant);
    let doc_id = "urn:example:aas:1:urn:example:sm:data";
    let doc_hash = hash_doc_id(doc_id);
    let topic = scheme.delta(&doc_hash);

    let mut sub_opts = MqttOptions::new(format!("sub-{}", Uuid::new_v4()), host.clone(), port);
    sub_opts.set_keep_alive(Duration::from_secs(5));
    let (sub_client, mut sub_eventloop) = AsyncClient::new(sub_opts, 10);
    sub_client
        .subscribe(&topic, QoS::AtLeastOnce)
        .await
        .unwrap();

    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        loop {
            match sub_eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    let _ = tx.send(publish.payload.to_vec());
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let mut pub_opts = MqttOptions::new(format!("pub-{}", Uuid::new_v4()), host, port);
    pub_opts.set_keep_alive(Duration::from_secs(5));
    let (pub_client, pub_eventloop) = AsyncClient::new(pub_opts, 10);
    tokio::spawn(spawn_eventloop(pub_eventloop));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut clock = Hlc::new(Uuid::new_v4());
    let ts = clock.tick();
    let mut delta = Delta::new();
    delta.add_insert("Temperature".to_string(), serde_json::json!(42), ts);

    let mut delta_payload = Vec::new();
    ciborium::into_writer(&delta, &mut delta_payload).unwrap();

    let doc_delta = DocDelta::new(doc_id.to_string(), ts, delta_payload);
    let payload = doc_delta.to_cbor().unwrap();

    pub_client
        .publish(&topic, QoS::AtLeastOnce, false, payload)
        .await
        .unwrap();

    let received = timeout(Duration::from_secs(5), rx)
        .await
        .expect("timeout waiting for MQTT message")
        .expect("subscriber dropped");

    let decoded = DocDelta::from_cbor(&received).unwrap();
    assert_eq!(decoded.doc_id, doc_id);
}
