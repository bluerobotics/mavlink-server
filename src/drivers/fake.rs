use anyhow::Result;
use mavlink::{ardupilotmega::MavMessage, read_v2_raw_message_async};
use tokio::sync::broadcast;
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
};

pub struct FakeSink;
pub struct FakeSource {
    pub period: std::time::Duration,
}

#[async_trait::async_trait]
impl Driver for FakeSink {
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()> {
        let mut hub_receiver = hub_sender.subscribe();

        while let Ok(message) = hub_receiver.recv().await {
            trace!("Message received: {message:?}")
        }

        Ok(())
    }

    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FakeSink".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Driver for FakeSource {
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()> {
        let mut sequence = 0;

        let mut buf: Vec<u8> = Vec::with_capacity(280);

        use mavlink::ardupilotmega::{
            MavAutopilot, MavMessage, MavModeFlag, MavState, MavType, HEARTBEAT_DATA,
        };

        loop {
            let header = mavlink::MavHeader {
                sequence,
                system_id: 1,
                component_id: 2,
            };
            let data = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                custom_mode: 5,
                mavtype: MavType::MAV_TYPE_QUADROTOR,
                autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                base_mode: MavModeFlag::MAV_MODE_FLAG_MANUAL_INPUT_ENABLED
                    | MavModeFlag::MAV_MODE_FLAG_STABILIZE_ENABLED
                    | MavModeFlag::MAV_MODE_FLAG_GUIDED_ENABLED
                    | MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED,
                system_status: MavState::MAV_STATE_STANDBY,
                mavlink_version: 3,
            });
            sequence = sequence.overflowing_add(1).0;

            buf.clear();
            mavlink::write_v2_msg(&mut buf, header, &data).expect("Failed to write message");

            let message = read_v2_raw_message_async::<MavMessage, _>(&mut (&buf[..]))
                .await
                .unwrap();

            trace!("Fake message created: {message:?}");

            let message = Protocol::new("", message);

            hub_sender.send(message).unwrap();

            tokio::time::sleep(self.period).await;
        }
    }

    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FakeSource".to_string(),
        }
    }
}
