use std::{net::Ipv4Addr, time::Duration};

use ptmp::{Call, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{devices::describe, paths::device},
    packet_tracer::{PacketTracer, PtError, expect_bool},
    server::PktctlServer,
};

const DEFAULT_PORT: &str = "FastEthernet0";
const IOS_KINDS: &[&str] = &[
    "router",
    "switch",
    "multi_layer_switch",
    "switch3650",
    "asa",
    "security_appliance",
];
const LEASE_POLLS: u32 = 10;
const LEASE_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HostConfigRequest {
    /// PC, laptop or server name.
    pub device: String,
    /// Network port to configure. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
    /// Get the address from a DHCP server instead of setting it statically.
    #[serde(default)]
    pub dhcp: bool,
    /// Static IPv4 address, for example `192.168.10.10`. Required unless `dhcp` is true.
    #[serde(default)]
    pub ip: Option<String>,
    /// Subnet mask, for example `255.255.255.0`. Required unless `dhcp` is true.
    #[serde(default)]
    pub mask: Option<String>,
    /// Default gateway; must be inside the host's subnet.
    #[serde(default)]
    pub gateway: Option<String>,
    /// DNS server address.
    #[serde(default)]
    pub dns: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HostConfig {
    pub device: String,
    pub port: String,
    pub dhcp: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct StaticAddress {
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    dns: Option<Ipv4Addr>,
}

enum Plan {
    Dhcp,
    Static(StaticAddress),
}

pub async fn configure<P: PacketTracer>(
    packet_tracer: &P,
    request: &HostConfigRequest,
) -> Result<HostConfig, PtError> {
    let plan = plan(request)?;
    let device_name = request.device.trim();
    let port_name = request
        .port
        .as_deref()
        .map(str::trim)
        .filter(|port| !port.is_empty())
        .unwrap_or(DEFAULT_PORT);
    let target = describe(packet_tracer, device_name).await?;
    if IOS_KINDS.contains(&target.kind.as_str()) {
        return Err(PtError::InvalidInput(format!(
            "`{device_name}` is a {}; configure_host only handles end devices such as PCs and \
             servers, use configure_ios for its interfaces",
            target.kind
        )));
    }
    let port = device(device_name).method("getPort", [Value::string(port_name)]);
    packet_tracer
        .call(port.clone().method("getName", []))
        .await
        .map_err(|error| match error {
            PtError::NotFound(_) => {
                PtError::NotFound(format!("port `{port_name}` on `{device_name}`"))
            }
            other => other,
        })?;

    let host = Host {
        packet_tracer,
        device: device(device_name),
        port,
    };
    match plan {
        Plan::Static(address) => host.apply_static(address).await?,
        Plan::Dhcp => host.apply_dhcp().await?,
    }

    let (dhcp, ip, mask) = host.read().await?;
    let (gateway, dns) = match plan {
        Plan::Static(address) => (address.gateway, address.dns),
        Plan::Dhcp => (None, None),
    };
    Ok(HostConfig {
        device: device_name.to_owned(),
        port: port_name.to_owned(),
        dhcp,
        ip: ip.map(|ip| ip.to_string()),
        mask: mask.map(|mask| mask.to_string()),
        gateway: gateway.map(|gateway| gateway.to_string()),
        dns: dns.map(|dns| dns.to_string()),
    })
}

struct Host<'a, P> {
    packet_tracer: &'a P,
    device: Call,
    port: Call,
}

impl<P: PacketTracer> Host<'_, P> {
    async fn apply_static(&self, address: StaticAddress) -> Result<(), PtError> {
        self.set_port("setDhcpClientFlag", [Value::Bool(false)])
            .await?;
        self.set_port(
            "setIpSubnetMask",
            [Value::Ip(address.ip), Value::Ip(address.mask)],
        )
        .await?;
        if let Some(gateway) = address.gateway {
            self.set_port("setDefaultGateway", [Value::Ip(gateway)])
                .await?;
        }
        if let Some(dns) = address.dns {
            self.set_port("setDnsServerIp", [Value::Ip(dns)]).await?;
        }
        Ok(())
    }

    async fn apply_dhcp(&self) -> Result<(), PtError> {
        self.set_port("setDhcpClientFlag", [Value::Bool(true)])
            .await?;
        self.packet_tracer
            .call(
                self.device
                    .clone()
                    .method("setDhcpFlag", [Value::Bool(true)]),
            )
            .await
            .map_err(explain_non_hosts)?;
        for _ in 0..LEASE_POLLS {
            if self.read().await?.1.is_some() {
                break;
            }
            tokio::time::sleep(LEASE_POLL_INTERVAL).await;
        }
        Ok(())
    }

    async fn read(&self) -> Result<(bool, Option<Ipv4Addr>, Option<Ipv4Addr>), PtError> {
        let get = |method: &'static str| {
            self.packet_tracer
                .call(self.port.clone().method(method, []))
        };
        let (dhcp, ip, mask) = tokio::try_join!(
            get("isDhcpClientOn"),
            get("getIpAddress"),
            get("getSubnetMask"),
        )?;
        let assigned = |value: &Value| value.as_ip().filter(|ip| !ip.is_unspecified());
        Ok((
            expect_bool(&dhcp, "DHCP flag")?,
            assigned(&ip),
            assigned(&mask),
        ))
    }

    async fn set_port(
        &self,
        method: &'static str,
        args: impl IntoIterator<Item = Value>,
    ) -> Result<(), PtError> {
        self.packet_tracer
            .call(self.port.clone().method(method, args))
            .await
            .map(drop)
            .map_err(explain_non_hosts)
    }
}

fn plan(request: &HostConfigRequest) -> Result<Plan, PtError> {
    if request.device.trim().is_empty() {
        return Err(PtError::InvalidInput("device is required".into()));
    }
    if request.dhcp {
        if request.ip.is_some() || request.mask.is_some() {
            return Err(PtError::InvalidInput(
                "use either dhcp or a static ip and mask, not both".into(),
            ));
        }
        return Ok(Plan::Dhcp);
    }

    let (Some(ip), Some(mask)) = (request.ip.as_deref(), request.mask.as_deref()) else {
        return Err(PtError::InvalidInput(
            "a static configuration needs both ip and mask (or set dhcp to true)".into(),
        ));
    };
    let ip = parse("ip", ip)?;
    let mask = parse("mask", mask)?;
    let prefix = prefix_length(mask)
        .ok_or_else(|| PtError::InvalidInput(format!("{mask} is not a valid subnet mask")))?;
    if prefix < 31 && (ip == network(ip, mask) || ip == broadcast(ip, mask)) {
        return Err(PtError::InvalidInput(format!(
            "{ip} is the network or broadcast address of {}/{prefix}",
            network(ip, mask)
        )));
    }
    let gateway = request
        .gateway
        .as_deref()
        .map(|gateway| parse("gateway", gateway))
        .transpose()?;
    if let Some(gateway) = gateway
        && network(gateway, mask) != network(ip, mask)
    {
        return Err(PtError::InvalidInput(format!(
            "gateway {gateway} is outside {}/{prefix}; a host can only reach a gateway in its own subnet",
            network(ip, mask)
        )));
    }
    let dns = request
        .dns
        .as_deref()
        .map(|dns| parse("dns", dns))
        .transpose()?;
    Ok(Plan::Static(StaticAddress {
        ip,
        mask,
        gateway,
        dns,
    }))
}

fn parse(field: &str, text: &str) -> Result<Ipv4Addr, PtError> {
    text.trim()
        .parse()
        .map_err(|_| PtError::InvalidInput(format!("{field} `{text}` is not an IPv4 address")))
}

fn prefix_length(mask: Ipv4Addr) -> Option<u32> {
    let bits = u32::from(mask);
    let ones = bits.leading_ones();
    (bits.checked_shl(ones).unwrap_or(0) == 0).then_some(ones)
}

fn network(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) & u32::from(mask))
}

fn broadcast(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(mask))
}

fn explain_non_hosts(error: PtError) -> PtError {
    match error {
        PtError::Rejected(reason) if reason.contains("not found") => PtError::Rejected(format!(
            "{reason}; this device cannot take an IPv4 configuration on that port"
        )),
        other => other,
    }
}

#[tool_router(router = hosts_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "configure_host",
        description = "Set the IPv4 configuration of a PC, laptop or server: either DHCP, or a \
                       static ip and mask with optional gateway and DNS. Catches classic \
                       mistakes (invalid mask, host address equal to the network or broadcast, \
                       gateway outside the subnet) before touching Packet Tracer.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn configure_host_tool(
        &self,
        Parameters(request): Parameters<HostConfigRequest>,
    ) -> Result<Json<HostConfig>, String> {
        configure(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        features::devices::{AddDeviceRequest, add},
        packet_tracer::scripted::ScriptedPacketTracer,
        testing::Canvas,
    };

    async fn lab() -> ScriptedPacketTracer {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(canvas);
        for (model, name) in [("PC-PT", "PC1"), ("2911", "R1")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        packet_tracer
    }

    fn static_request(ip: &str, mask: &str, gateway: Option<&str>) -> HostConfigRequest {
        HostConfigRequest {
            device: "PC1".into(),
            ip: Some(ip.into()),
            mask: Some(mask.into()),
            gateway: gateway.map(str::to_owned),
            ..HostConfigRequest::default()
        }
    }

    #[tokio::test]
    async fn applies_and_reads_back_a_static_address() {
        let packet_tracer = lab().await;
        let request = HostConfigRequest {
            dns: Some("192.168.10.53".into()),
            ..static_request("192.168.10.10", "255.255.255.0", Some("192.168.10.1"))
        };
        assert_eq!(
            configure(&packet_tracer, &request).await.unwrap(),
            HostConfig {
                device: "PC1".into(),
                port: "FastEthernet0".into(),
                dhcp: false,
                ip: Some("192.168.10.10".into()),
                mask: Some("255.255.255.0".into()),
                gateway: Some("192.168.10.1".into()),
                dns: Some("192.168.10.53".into()),
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn switches_to_dhcp_and_reports_the_missing_lease() {
        let packet_tracer = lab().await;
        configure(
            &packet_tracer,
            &static_request("10.0.0.5", "255.0.0.0", None),
        )
        .await
        .unwrap();
        let request = HostConfigRequest {
            device: "PC1".into(),
            dhcp: true,
            ..HostConfigRequest::default()
        };
        let config = configure(&packet_tracer, &request).await.unwrap();
        assert!(config.dhcp);
        assert_eq!(config.ip, None);
    }

    #[tokio::test]
    async fn catches_classic_addressing_mistakes() {
        let packet_tracer = lab().await;
        let cases = [
            (
                static_request("192.168.10.10", "255.0.255.0", None),
                "not a valid subnet mask",
            ),
            (
                static_request("192.168.10.0", "255.255.255.0", None),
                "network or broadcast",
            ),
            (
                static_request("192.168.10.255", "255.255.255.0", None),
                "network or broadcast",
            ),
            (
                static_request("192.168.10.10", "255.255.255.0", Some("192.168.20.1")),
                "outside 192.168.10.0/24",
            ),
            (
                static_request("192.168.10.300", "255.255.255.0", None),
                "not an IPv4 address",
            ),
        ];
        for (request, expected) in cases {
            let error = configure(&packet_tracer, &request).await.unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        let both = HostConfigRequest {
            dhcp: true,
            ..static_request("192.168.10.10", "255.255.255.0", None)
        };
        assert!(configure(&packet_tracer, &both).await.is_err());
    }

    #[tokio::test]
    async fn points_routers_to_ios_configuration() {
        let packet_tracer = lab().await;
        let request = HostConfigRequest {
            device: "R1".into(),
            port: Some("GigabitEthernet0/0".into()),
            ..static_request("10.0.0.1", "255.0.0.0", None)
        };
        let error = configure(&packet_tracer, &request).await.unwrap_err();
        assert!(error.to_string().contains("configure_ios"), "{error}");
    }

    #[test]
    fn prefix_lengths() {
        assert_eq!(prefix_length(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(prefix_length(Ipv4Addr::new(255, 255, 255, 252)), Some(30));
        assert_eq!(prefix_length(Ipv4Addr::UNSPECIFIED), Some(0));
        assert_eq!(prefix_length(Ipv4Addr::BROADCAST), Some(32));
        assert_eq!(prefix_length(Ipv4Addr::new(255, 0, 255, 0)), None);
    }
}
