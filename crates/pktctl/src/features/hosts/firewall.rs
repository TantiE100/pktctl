use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::host_port;
use crate::{
    features::paths::device,
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_integer, expect_text},
};

/// Packet Tracer keeps a host's firewall rules in the ACL numbered 101 of its ACL process.
const FIREWALL_ACL: &str = "101";
const IPV4_PROCESS: &str = "AclProcess";
const IPV6_PROCESS: &str = "Aclv6Process";
const ANY_IPV4: (&str, &str) = ("0.0.0.0", "255.255.255.255");
const ANY_IPV6: (&str, &str) = ("::", "0");
const PROTOCOLS: &[&str] = &["ip", "icmp", "tcp", "udp"];
const PORTED_PROTOCOLS: &[&str] = &["tcp", "udp"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    #[default]
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Permit,
    Deny,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FirewallRule {
    /// `ipv4` (default) or `ipv6`.
    #[serde(default)]
    pub family: Family,
    /// `permit` or `deny`.
    pub action: RuleAction,
    /// `ip`, `icmp`, `tcp` or `udp`.
    pub protocol: String,
    /// Remote address the rule matches. Defaults to any.
    #[serde(default)]
    pub remote_ip: Option<String>,
    /// IPv4: wildcard mask (`0.0.0.0` for one host, `255.255.255.255` for any).
    /// IPv6: prefix length (`128` for one host, `0` for any).
    #[serde(default)]
    pub remote_mask: Option<String>,
    /// Port for tcp and udp, for example 80. Packet Tracer keeps one port per rule.
    #[serde(default)]
    pub port: Option<i32>,
}

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
    /// Rules to add, in the order the firewall will evaluate them.
    #[serde(default)]
    pub add_rules: Vec<FirewallRule>,
    /// Rules to remove; they must match an existing rule exactly.
    #[serde(default)]
    pub remove_rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FirewallState {
    pub device: String,
    pub port: String,
    pub ipv4: bool,
    pub ipv6: bool,
    /// IPv4 rules as the firewall lists them, in order.
    pub ipv4_rules: Vec<String>,
    pub ipv6_rules: Vec<String>,
}

pub async fn set_firewall<P: PacketTracer>(
    packet_tracer: &P,
    request: &FirewallRequest,
) -> Result<FirewallState, PtError> {
    let edits: Vec<(bool, &FirewallRule)> = request
        .add_rules
        .iter()
        .map(|rule| (true, rule))
        .chain(request.remove_rules.iter().map(|rule| (false, rule)))
        .collect();
    for (_, rule) in &edits {
        check(rule)?;
    }
    let (device_name, port_name, port) =
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
    for (add, rule) in edits {
        apply(packet_tracer, &device_name, add, rule).await?;
    }

    let (ipv4, ipv6) = tokio::try_join!(
        packet_tracer.call(port.clone().method("isInboundFirewallOn", [])),
        packet_tracer.call(port.clone().method("isInboundIpv6FirewallOn", [])),
    )?;
    Ok(FirewallState {
        ipv4: expect_bool(&ipv4, "isInboundFirewallOn")?,
        ipv6: expect_bool(&ipv6, "isInboundIpv6FirewallOn")?,
        ipv4_rules: rules(packet_tracer, &device_name, Family::Ipv4).await?,
        ipv6_rules: rules(packet_tracer, &device_name, Family::Ipv6).await?,
        device: device_name,
        port: port_name,
    })
}

fn check(rule: &FirewallRule) -> Result<(), PtError> {
    let protocol = rule.protocol.trim().to_lowercase();
    if !PROTOCOLS.contains(&protocol.as_str()) {
        return Err(PtError::InvalidInput(format!(
            "protocol `{}` is not one of {}",
            rule.protocol,
            PROTOCOLS.join(", ")
        )));
    }
    let port = rule.port.unwrap_or_default();
    if !PORTED_PROTOCOLS.contains(&protocol.as_str()) && port != 0 {
        return Err(PtError::InvalidInput(format!(
            "a port only applies to tcp and udp, not {protocol}"
        )));
    }
    if !(0..=65535).contains(&port) {
        return Err(PtError::InvalidInput(
            "port must be between 0 and 65535".into(),
        ));
    }
    Ok(())
}

async fn apply<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    add: bool,
    rule: &FirewallRule,
) -> Result<(), PtError> {
    let acl = acl(packet_tracer, device_name, rule.family).await?;
    let method = if add {
        "addExtStatement"
    } else {
        "removeExtStatement"
    };
    let (any_ip, any_mask) = match rule.family {
        Family::Ipv4 => ANY_IPV4,
        Family::Ipv6 => ANY_IPV6,
    };
    let text = |value: &Option<String>, fallback: &str| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    let done = packet_tracer
        .call(acl.method(
            method,
            [
                Value::Bool(rule.family == Family::Ipv6),
                Value::string(rule.protocol.trim().to_lowercase()),
                Value::Bool(rule.action == RuleAction::Permit),
                Value::string(text(&rule.remote_ip, any_ip)),
                Value::string(text(&rule.remote_mask, any_mask)),
                Value::Int(0),
                Value::string(any_ip),
                Value::string(any_mask),
                // Packet Tracer stores and matches one port, the last argument, whichever
                // of the two the Firewall app filled in.
                Value::Int(rule.port.unwrap_or_default()),
            ],
        ))
        .await?;
    if !expect_bool(&done, method)? {
        return Err(PtError::Rejected(format!(
            "Packet Tracer would not {} that rule on `{device_name}`{}",
            if add { "add" } else { "remove" },
            if add {
                ""
            } else {
                "; it must match an existing one exactly"
            }
        )));
    }
    Ok(())
}

async fn rules<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    family: Family,
) -> Result<Vec<String>, PtError> {
    let acl = acl(packet_tracer, device_name, family).await?;
    let count = packet_tracer
        .call(acl.clone().method("getCommandCount", []))
        .await?;
    let count = i32::try_from(expect_integer(&count, "rule count")?)
        .map_err(|_| PtError::UnexpectedReply("rule count out of range".into()))?;
    let mut listed = Vec::new();
    for index in 0..count {
        let rule = packet_tracer
            .call(acl.clone().method("getCommandAt", [Value::Int(index)]))
            .await?;
        listed.push(expect_text(&rule, "firewall rule")?);
    }
    Ok(listed)
}

/// The firewall's ACL, created on first use the way the Firewall app does.
async fn acl<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    family: Family,
) -> Result<Call, PtError> {
    let process = device(device_name).method(
        "getProcess",
        [Value::string(match family {
            Family::Ipv4 => IPV4_PROCESS,
            Family::Ipv6 => IPV6_PROCESS,
        })],
    );
    let acl = process
        .clone()
        .method("getAcl", [Value::string(FIREWALL_ACL)]);
    if packet_tracer
        .call(acl.clone().method("getCommandCount", []))
        .await
        .is_err()
    {
        packet_tracer
            .call(
                process
                    .clone()
                    .method("addAcl", [Value::string(FIREWALL_ACL)]),
            )
            .await?;
    }
    Ok(acl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(protocol: &str, port: Option<i32>) -> FirewallRule {
        FirewallRule {
            family: Family::Ipv4,
            action: RuleAction::Deny,
            protocol: protocol.into(),
            remote_ip: None,
            remote_mask: None,
            port,
        }
    }

    #[test]
    fn checks_rules_before_sending_them() {
        assert!(check(&rule("icmp", None)).is_ok());
        assert!(check(&rule("TCP", Some(80))).is_ok());
        let protocol = check(&rule("arp", None)).unwrap_err().to_string();
        assert!(protocol.contains("ip, icmp, tcp, udp"), "{protocol}");
        let ports = check(&rule("icmp", Some(80))).unwrap_err().to_string();
        assert!(ports.contains("only applies to tcp and udp"), "{ports}");
        assert!(check(&rule("tcp", Some(70000))).is_err());
    }
}
