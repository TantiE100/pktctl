use std::{net::Ipv6Addr, time::Duration};

use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::host_port;
use crate::packet_tracer::{PacketTracer, PtError, expect_bool};

const UNICAST: i32 = 0;
const AUTO_CONFIG_POLLS: u32 = 10;
const AUTO_CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ipv6Mode {
    /// A fixed address with `address`, like the Static button of IP Configuration.
    #[default]
    Static,
    /// Stateless autoconfiguration (SLAAC) from the router's advertisements.
    Auto,
    /// IPv6 switched off on the port.
    Off,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Ipv6Request {
    /// PC, laptop or server name.
    pub device: String,
    /// Network port to configure. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
    /// `static` (default), `auto` for SLAAC, or `off`.
    #[serde(default)]
    pub mode: Ipv6Mode,
    /// Static address with its prefix length, for example `2001:db8:10::20/64`.
    #[serde(default)]
    pub address: Option<String>,
    /// Default gateway, usually the router's link-local address such as `fe80::1`.
    #[serde(default)]
    pub gateway: Option<String>,
    /// IPv6 DNS server.
    #[serde(default)]
    pub dns: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Ipv6Config {
    pub device: String,
    pub port: String,
    pub mode: Ipv6Mode,
    /// Addresses on the port as `address/prefix`, read back from Packet Tracer.
    pub addresses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<String>,
}

struct Plan {
    address: Option<(Ipv6Addr, i32)>,
    gateway: Option<Ipv6Addr>,
    dns: Option<Ipv6Addr>,
}

pub async fn configure_ipv6<P: PacketTracer>(
    packet_tracer: &P,
    request: &Ipv6Request,
) -> Result<Ipv6Config, PtError> {
    let plan = plan(request)?;
    let (device, port_name, port) =
        host_port(packet_tracer, &request.device, request.port.as_deref()).await?;
    let set = |method: &'static str, args: Vec<Value>| {
        packet_tracer.call(port.clone().method(method, args))
    };

    set(
        "setIpv6Enabled",
        vec![Value::Bool(request.mode != Ipv6Mode::Off)],
    )
    .await?;
    match request.mode {
        Ipv6Mode::Off => {
            set("removeAllIpv6Addresses", vec![]).await?;
        }
        Ipv6Mode::Auto => {
            set("removeAllIpv6Addresses", vec![]).await?;
            set("setIpv6AddressAutoConfig", vec![Value::Bool(true)]).await?;
        }
        Ipv6Mode::Static => {
            set("setIpv6AddressAutoConfig", vec![Value::Bool(false)]).await?;
            set("removeAllIpv6Addresses", vec![]).await?;
            if let Some((address, prefix)) = plan.address {
                let added = set(
                    "addIpv6Address",
                    vec![
                        Value::Ipv6(address),
                        Value::Int(prefix),
                        Value::Int(UNICAST),
                        Value::Bool(false),
                    ],
                )
                .await?;
                if !expect_bool(&added, "addIpv6Address")? {
                    return Err(PtError::Rejected(format!(
                        "Packet Tracer did not accept {address}/{prefix} on {port_name}"
                    )));
                }
            }
        }
    }
    if let Some(gateway) = plan.gateway {
        set("setv6DefaultGateway", vec![Value::Ipv6(gateway)]).await?;
    }
    if let Some(dns) = plan.dns {
        set("setv6ServerIp", vec![Value::Ipv6(dns)]).await?;
    }

    let mut addresses = read_addresses(packet_tracer, &port).await?;
    if request.mode == Ipv6Mode::Auto {
        for _ in 0..AUTO_CONFIG_POLLS {
            if !addresses.is_empty() {
                break;
            }
            tokio::time::sleep(AUTO_CONFIG_POLL_INTERVAL).await;
            addresses = read_addresses(packet_tracer, &port).await?;
        }
    }
    Ok(Ipv6Config {
        device,
        port: port_name,
        mode: request.mode,
        addresses,
        gateway: plan.gateway.map(|gateway| gateway.to_string()),
        dns: plan.dns.map(|dns| dns.to_string()),
    })
}

async fn read_addresses<P: PacketTracer>(
    packet_tracer: &P,
    port: &Call,
) -> Result<Vec<String>, PtError> {
    let list = packet_tracer
        .call(port.clone().method("getIpv6Addresses", []))
        .await?;
    let Value::Vector { items, .. } = list else {
        return Err(PtError::UnexpectedReply(format!(
            "getIpv6Addresses should return a list, got {list:?}"
        )));
    };
    Ok(items
        .into_iter()
        .filter_map(|item| match item {
            Value::Data { fields, .. } => {
                let address = fields.first()?.as_ipv6()?;
                let prefix = match fields.get(1)? {
                    Value::String(text) | Value::QString(text) => text.parse().ok()?,
                    other => other.as_i64()?,
                };
                Some(format!("{address}/{prefix}"))
            }
            _ => None,
        })
        .collect())
}

fn plan(request: &Ipv6Request) -> Result<Plan, PtError> {
    let address = match (request.mode, request.address.as_deref().map(str::trim)) {
        (Ipv6Mode::Static, Some(text)) => Some(parse_address(text)?),
        (Ipv6Mode::Static, None) => {
            return Err(PtError::InvalidInput(
                "a static IPv6 configuration needs `address`, for example 2001:db8:10::20/64"
                    .into(),
            ));
        }
        (_, Some(_)) => {
            return Err(PtError::InvalidInput(
                "`address` only goes with mode `static`".into(),
            ));
        }
        (_, None) => None,
    };
    let optional =
        |field: &str, text: Option<&str>| text.map(|text| parse(field, text.trim())).transpose();
    Ok(Plan {
        address,
        gateway: optional("gateway", request.gateway.as_deref())?,
        dns: optional("dns", request.dns.as_deref())?,
    })
}

fn parse_address(text: &str) -> Result<(Ipv6Addr, i32), PtError> {
    let (address, prefix) = text.split_once('/').ok_or_else(|| {
        PtError::InvalidInput(format!(
            "`{text}` needs a prefix length, for example 2001:db8:10::20/64"
        ))
    })?;
    let address = parse("address", address)?;
    let prefix: i32 = prefix
        .parse()
        .ok()
        .filter(|prefix| (1..=128).contains(prefix))
        .ok_or_else(|| {
            PtError::InvalidInput(format!("prefix `/{prefix}` must be between 1 and 128"))
        })?;
    if address.is_unspecified() || address.is_multicast() || address.is_unicast_link_local() {
        return Err(PtError::InvalidInput(format!(
            "{address} cannot be a host's address; use a global or unique local address \
             (link-local addresses are set automatically)"
        )));
    }
    Ok((address, prefix))
}

fn parse(field: &str, text: &str) -> Result<Ipv6Addr, PtError> {
    text.parse()
        .map_err(|_| PtError::InvalidInput(format!("{field} `{text}` is not an IPv6 address")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: Ipv6Mode, address: Option<&str>) -> Ipv6Request {
        Ipv6Request {
            device: "PC1".into(),
            mode,
            address: address.map(Into::into),
            ..Ipv6Request::default()
        }
    }

    #[test]
    fn checks_addresses_before_touching_packet_tracer() {
        assert!(plan(&request(Ipv6Mode::Static, Some("2001:db8::20/64"))).is_ok());
        for (address, complaint) in [
            ("2001:db8::20", "prefix length"),
            ("2001:db8::20/129", "between 1 and 128"),
            ("fe80::20/64", "link-local"),
            ("ff02::1/64", "cannot be a host"),
            ("2001:zz8::20/64", "not an IPv6 address"),
        ] {
            let error = plan(&request(Ipv6Mode::Static, Some(address)))
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(complaint), "{address}: {error}");
        }
        assert!(plan(&request(Ipv6Mode::Static, None)).is_err());
        assert!(plan(&request(Ipv6Mode::Auto, Some("2001:db8::20/64"))).is_err());
        assert!(plan(&request(Ipv6Mode::Auto, None)).is_ok());
    }
}
