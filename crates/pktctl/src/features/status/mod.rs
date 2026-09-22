use ptmp::Call;
use rmcp::{Json, tool, tool_router};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    packet_tracer::{PacketTracer, PtError, expect_integer},
    server::PktctlServer,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Status {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pt_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

pub async fn check<P: PacketTracer>(packet_tracer: &P) -> Status {
    let counts = async {
        tokio::try_join!(
            packet_tracer.version(),
            count(packet_tracer, "getDeviceCount"),
            count(packet_tracer, "getLinkCount"),
        )
    };
    match counts.await {
        Ok((version, devices, links)) => Status {
            connected: true,
            pt_version: Some(version),
            devices: Some(devices),
            links: Some(links),
            problem: None,
        },
        Err(error) => Status {
            connected: false,
            pt_version: None,
            devices: None,
            links: None,
            problem: Some(error.to_string()),
        },
    }
}

async fn count<P: PacketTracer>(packet_tracer: &P, method: &str) -> Result<i64, PtError> {
    let value = packet_tracer
        .call(Call::root("network").method(method, []))
        .await?;
    expect_integer(&value, method)
}

#[tool_router(router = status_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "status",
        description = "Check whether Packet Tracer is reachable and summarize the open network. \
                       When it is not reachable, `problem` explains what to fix.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn status_tool(&self) -> Json<Status> {
        Json(check(self.packet_tracer()).await)
    }
}

#[cfg(test)]
mod tests {
    use ptmp::Value;

    use super::*;
    use crate::packet_tracer::scripted::{ScriptedPacketTracer, methods};

    #[tokio::test]
    async fn summarizes_a_reachable_network() {
        let packet_tracer = ScriptedPacketTracer::new(|call| {
            Ok(match methods(call)[1] {
                "getDeviceCount" => Value::Int(11),
                _ => Value::Int(9),
            })
        });
        assert_eq!(
            check(&packet_tracer).await,
            Status {
                connected: true,
                pt_version: Some("9.0.1.0858".into()),
                devices: Some(11),
                links: Some(9),
                problem: None,
            }
        );
    }

    #[tokio::test]
    async fn explains_why_packet_tracer_is_unreachable() {
        let status = check(&ScriptedPacketTracer::unreachable()).await;
        assert!(!status.connected);
        assert!(status.problem.unwrap().contains("open Packet Tracer"));
    }
}
