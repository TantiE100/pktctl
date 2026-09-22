use futures::future::try_join_all;
use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    features::{
        devices::describe,
        paths::{device, network},
    },
    packet_tracer::{
        PacketTracer, PtError, expect_bool, expect_integer, expect_text, kinds::cable_kind,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Endpoint {
    pub device: String,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Connection {
    pub to: Endpoint,
    pub cable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Port {
    pub name: String,
    pub up: bool,
    pub protocol_up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PortList {
    pub device: String,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Link {
    pub a: Endpoint,
    pub b: Endpoint,
    pub cable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LinkList {
    pub links: Vec<Link>,
}

pub async fn list_ports<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
) -> Result<PortList, PtError> {
    let device_name = device_name.trim();
    describe(packet_tracer, device_name).await?;
    let count = packet_tracer
        .call(device(device_name).method("getPortCount", []))
        .await?;
    let count = i32::try_from(expect_integer(&count, "port count")?)
        .map_err(|_| PtError::UnexpectedReply("port count out of range".into()))?;
    let ports = try_join_all((0..count).map(|index| {
        read_port(
            packet_tracer,
            device_name,
            device(device_name).method("getPortAt", [Value::Int(index)]),
        )
    }))
    .await?;
    Ok(PortList {
        device: device_name.to_owned(),
        ports,
    })
}

pub async fn port_names<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
) -> Result<Vec<String>, PtError> {
    Ok(list_ports(packet_tracer, device_name)
        .await?
        .ports
        .into_iter()
        .map(|port| port.name)
        .collect())
}

pub async fn connection<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    port_name: &str,
) -> Result<Option<Connection>, PtError> {
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
    read_connection(packet_tracer, device_name, port_name, &port).await
}

pub async fn list_links<P: PacketTracer>(packet_tracer: &P) -> Result<LinkList, PtError> {
    let count = packet_tracer
        .call(network().method("getLinkCount", []))
        .await?;
    let count = i32::try_from(expect_integer(&count, "link count")?)
        .map_err(|_| PtError::UnexpectedReply("link count out of range".into()))?;
    let links = try_join_all((0..count).map(|index| async move {
        let link = network().method("getLinkAt", [Value::Int(index)]);
        let (a, b, cable) = tokio::try_join!(
            end(packet_tracer, &link, "getPort1"),
            end(packet_tracer, &link, "getPort2"),
            packet_tracer.call(link.clone().method("getConnectionType", [])),
        )?;
        Ok::<_, PtError>(Link {
            a,
            b,
            cable: cable_kind(expect_integer(&cable, "cable type")?),
        })
    }))
    .await?;
    Ok(LinkList { links })
}

async fn read_port<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    port: Call,
) -> Result<Port, PtError> {
    let get = |method: &'static str| packet_tracer.call(port.clone().method(method, []));
    let (name, up, protocol_up, ip, mask) = tokio::join!(
        get("getName"),
        get("isPortUp"),
        get("isProtocolUp"),
        get("getIpAddress"),
        get("getSubnetMask"),
    );
    let name = expect_text(&name?, "port name")?;
    let connection = read_connection(packet_tracer, device_name, &name, &port).await?;
    Ok(Port {
        up: expect_bool(&up?, "port status")?,
        protocol_up: expect_bool(&protocol_up?, "protocol status")?,
        ip: address(ip)?,
        mask: address(mask)?,
        connection,
        name,
    })
}

async fn read_connection<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    port_name: &str,
    port: &Call,
) -> Result<Option<Connection>, PtError> {
    let link = port.clone().method("getLink", []);
    let cable = match packet_tracer
        .call(link.clone().method("getConnectionType", []))
        .await
    {
        Ok(cable) => cable_kind(expect_integer(&cable, "cable type")?),
        Err(PtError::NotFound(_)) => return Ok(None),
        Err(other) => return Err(other),
    };
    let (first, second) = tokio::try_join!(
        end(packet_tracer, &link, "getPort1"),
        end(packet_tracer, &link, "getPort2"),
    )?;
    let to = if first.device == device_name && first.port == port_name {
        second
    } else {
        first
    };
    Ok(Some(Connection { to, cable }))
}

async fn end<P: PacketTracer>(
    packet_tracer: &P,
    link: &Call,
    which: &'static str,
) -> Result<Endpoint, PtError> {
    let port = link.clone().method(which, []);
    let (device, name) = tokio::try_join!(
        packet_tracer.call(
            port.clone()
                .method("getOwnerDevice", [])
                .method("getName", [])
        ),
        packet_tracer.call(port.clone().method("getName", [])),
    )?;
    Ok(Endpoint {
        device: expect_text(&device, "link device")?,
        port: expect_text(&name, "link port")?,
    })
}

fn address(reply: Result<Value, PtError>) -> Result<Option<String>, PtError> {
    match reply {
        Ok(value) => Ok(value
            .as_ip()
            .filter(|address| !address.is_unspecified())
            .map(|address| address.to_string())),
        Err(PtError::Rejected(reason)) if reason.contains("not found") => Ok(None),
        Err(other) => Err(other),
    }
}
