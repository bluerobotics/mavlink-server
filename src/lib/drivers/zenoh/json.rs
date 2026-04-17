use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use mavlink::{self, Message};
use tokio::sync::{RwLock, broadcast};
use tracing::*;
use zenoh;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{Driver, DriverInfo, generic_tasks::SendReceiveContext, zenoh::session},
    mavlink_json::MAVLinkJSON,
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

const TOPIC_PREFIX: &str = "mavlink";
const DRIVER_IDENTIFIER: &str = "zenoh";

#[derive(Debug)]
pub struct Zenoh {
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct ZenohBuilder(Zenoh);

impl ZenohBuilder {
    pub fn build(self) -> Zenoh {
        self.0
    }

    pub fn on_message_input<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_input.add_callback(callback.into_boxed());
        self
    }

    pub fn on_message_output<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_output.add_callback(callback.into_boxed());
        self
    }
}

impl Zenoh {
    #[instrument(level = "debug")]
    pub fn builder(name: &str) -> ZenohBuilder {
        let name = Arc::new(name.to_string());

        ZenohBuilder(Self {
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&name),
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(name, &ZenohInfo))),
        })
    }

    #[instrument(level = "debug", skip_all)]
    async fn receive_task(
        context: &SendReceiveContext,
        session: Arc<zenoh::Session>,
    ) -> Result<()> {
        let subscriber = match session
            .declare_subscriber(format!("{TOPIC_PREFIX}/in"))
            .await
        {
            Ok(subscriber) => subscriber,
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "Failed to create subscriber for mavlink data: {error:?}"
                ));
            }
        };

        let origin: Arc<str> = Arc::from(DRIVER_IDENTIFIER);

        'mainloop: loop {
            let sample = match subscriber.recv_async().await {
                Ok(sample) => sample,
                Err(error) => {
                    error!("Failed to receive sample: no senders left: {error:?}");
                    break;
                }
            };

            let content = match json5::from_str::<MAVLinkJSON<mavlink::ardupilotmega::MavMessage>>(
                std::str::from_utf8(&sample.payload().to_bytes()).unwrap(),
            ) {
                Ok(content) => content,
                Err(error) => {
                    debug!(
                        "Failed to parse message, not a valid MAVLinkMessage: {sample:?}. Error: {error:?}"
                    );
                    continue;
                }
            };

            let bus_message = Arc::new(Protocol::from_mavlink_raw(
                content.header.inner,
                &content.message,
                Arc::clone(&origin),
            ));

            trace!("Received message: {bus_message:?}");

            context.stats.write().await.stats.update_input(&bus_message);

            for future in context.on_message_input.call_all(bus_message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue 'mainloop;
                }
            }

            if let Err(error) = context.hub_sender.send(bus_message) {
                error!("Failed to send message to hub: {error:?}");
                continue;
            }

            trace!("Message sent to hub");
        }

        debug!("Driver receiver task stopped!");

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    async fn send_task(context: &SendReceiveContext, session: Arc<zenoh::Session>) -> Result<()> {
        let mut hub_receiver = context.hub_sender.subscribe();
        let mut publishers = HashMap::new();

        'mainloop: loop {
            let message = match hub_receiver.recv().await {
                Ok(message) => message,
                Err(broadcast::error::RecvError::Closed) => {
                    error!("Hub channel closed!");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!("Channel lagged by {count} messages.");
                    continue;
                }
            };

            if message.origin.as_ref().eq(DRIVER_IDENTIFIER) {
                continue; // Don't do loopback
            }

            context.stats.write().await.stats.update_output(&message);

            for future in context.on_message_output.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!(
                        "Dropping message: on_message_output callback returned error: {error:?}"
                    );
                    continue 'mainloop;
                }
            }

            let mavlink_json = match message
                .to_mavlink_json::<mavlink::ardupilotmega::MavMessage>()
                .await
            {
                Ok(mavlink_json) => mavlink_json,
                Err(error) => {
                    error!("Failed converting to mavlink json: {error:?}");
                    continue;
                }
            };

            let message_name = mavlink_json.message.message_name();

            let json_string = &match json5::to_string(&mavlink_json) {
                Ok(json) => json,
                Err(error) => {
                    error!("Failed to transform mavlink message {message_name} to json: {error:?}");
                    continue;
                }
            };

            let out_topic_name = format!("{TOPIC_PREFIX}/out");
            Self::publish_json(&session, &mut publishers, &out_topic_name, json_string).await;

            let header = &mavlink_json.header.inner;
            let message_topic_name = format!(
                "mavlink/{}/{}/{}",
                header.system_id, header.component_id, message_name
            );
            Self::publish_json(&session, &mut publishers, &message_topic_name, json_string).await;

            // for each key inside mavlink_json and publish under topic_name/field_name
            let message_value = serde_json::to_value(&mavlink_json.message).unwrap();
            for (field_name, field_value) in message_value.as_object().unwrap() {
                let field_topic_name = format!("{message_topic_name}/{field_name}");
                let field_json = json5::to_string(field_value).unwrap();
                Self::publish_json(&session, &mut publishers, &field_topic_name, &field_json).await;
            }
        }

        debug!("Driver sender task stopped!");

        Ok(())
    }

    async fn publish_json(
        session: &zenoh::Session,
        publishers: &mut HashMap<String, zenoh::pubsub::Publisher<'static>>,
        topic_name: &str,
        payload: &str,
    ) {
        if !publishers.contains_key(topic_name) {
            let topic = topic_name.to_string();
            let key_expr = match zenoh::key_expr::KeyExpr::try_from(topic.clone()) {
                Ok(key_expr) => key_expr,
                Err(error) => {
                    error!("Failed to create key expression for {topic_name}: {error:?}");
                    return;
                }
            };

            match session
                .declare_publisher(key_expr)
                .encoding(zenoh::bytes::Encoding::APPLICATION_JSON.with_schema("mavlink"))
                .congestion_control(zenoh::qos::CongestionControl::Block)
                .priority(zenoh::qos::Priority::RealTime)
                .express(false)
                .await
            {
                Ok(publisher) => {
                    publishers.insert(topic, publisher);
                }
                Err(error) => {
                    error!("Failed to create publisher for {topic_name}: {error:?}");
                    return;
                }
            }
        }

        let Some(publisher) = publishers.get(topic_name) else {
            return;
        };

        if let Err(error) = publisher
            .put(payload)
            .encoding(zenoh::bytes::Encoding::APPLICATION_JSON)
            .await
        {
            error!("Failed to send message to {topic_name}: {error:?}");
        } else {
            trace!("Message sent to {topic_name}: {payload:?}");
        }
    }
}

#[async_trait::async_trait]
impl Driver for Zenoh {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let context = SendReceiveContext {
            direction: crate::drivers::Direction::Both,
            hub_sender,
            on_message_output: self.on_message_output.clone(),
            on_message_input: self.on_message_input.clone(),
            stats: self.stats.clone(),
        };

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            debug!("Trying to connect...");

            let session = session::session().await;

            debug!("Successfully connected");

            tokio::select! {
                result = Zenoh::send_task(&context, session.clone()) => {
                    if let Err(error) = result {
                        error!("Error in send task: {error:?}");
                    }
                }
                result = Zenoh::receive_task(&context, session) => {
                    if let Err(error) = result {
                        error!("Error in receive task: {error:?}");
                    }
                }
            }

            debug!("Restarting connection loop...");
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(ZenohInfo)
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for Zenoh {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct ZenohInfo;
impl DriverInfo for ZenohInfo {
    fn name(&self) -> &'static str {
        "Zenoh"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["zenoh"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = self.valid_schemes()[0];

        vec![format!("{first_schema}:<IP>:<PORT>")]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![format!("{first_schema}://<IP>:<PORT>").to_string()]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        println!("{}", &url);
        let _host = url.host_str().unwrap();
        let _port = url.port().unwrap();
        Some(Arc::new(Zenoh::builder("Zenoh").build()))
    }
}
