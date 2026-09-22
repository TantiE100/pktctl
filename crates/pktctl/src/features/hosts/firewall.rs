use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::host_port;
use crate::packet_tracer::{PacketTracer, PtError, expect_bool};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FirewallRequest {
    /// PC, laptop or server name.
    pub device: String,
    /// Network port. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
    /// Switch the IPv4 inbound firewall on or off. Omit to leave it as it is.
    #[serde(default)]
    pub ipv4: Option<bool>,
    /// Switch the IPv6 inbound firewall on or off. Omit to leave it as it is.
    #[serde(default)]
    pub ipv6: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FirewallState {
    pub device: String,
    pub port: String,
    pub ipv4: bool,
    pub ipv6: bool,
}

pub async fn set_firewall<P: PacketTracer>(
    packet_tracer: &P,
    request: &FirewallRequest,
) -> Result<FirewallState, PtError> {
    let (device, port_name, port) =
        host_port(packet_tracer, &request.device, request.port.as_deref()).await?;
    for (setter, wanted) in [
        ("setInboundFirewallService", request.ipv4),
        ("setInboundIpv6FirewallService", request.ipv6),
    ] {
        if let Some(on) = wanted {
            packet_tracer
                .call(port.clone().method(setter, [Value::Bool(on)]))
                .await?;
        }
    }
    let (ipv4, ipv6) = tokio::try_join!(
        packet_tracer.call(port.clone().method("isInboundFirewallOn", [])),
        packet_tracer.call(port.clone().method("isInboundIpv6FirewallOn", [])),
    )?;
    Ok(FirewallState {
        device,
        port: port_name,
        ipv4: expect_bool(&ipv4, "isInboundFirewallOn")?,
        ipv6: expect_bool(&ipv6, "isInboundIpv6FirewallOn")?,
    })
}
