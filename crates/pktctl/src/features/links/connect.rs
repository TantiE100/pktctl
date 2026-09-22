use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    cabling::Cable,
    ports::{Connection, Endpoint, Link, connection, port_names},
};
use crate::{
    features::{devices::describe, paths::logical_workspace},
    packet_tracer::{PacketTracer, PtError, expect_bool},
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ConnectRequest {
    /// First device name.
    pub device_a: String,
    /// Port on the first device, exactly as `list_ports` shows it, for example `GigabitEthernet0/0`.
    pub port_a: String,
    /// Second device name.
    pub device_b: String,
    /// Port on the second device.
    pub port_b: String,
    /// Cable to use. `auto` picks serial for serial ports, straight between a host or router
    /// and a switch, and cross between devices of the same layer.
    #[serde(default)]
    pub cable: Cable,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PortRef {
    /// Device name.
    pub device: String,
    /// Port name, exactly as `list_ports` shows it.
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Disconnected {
    pub device: String,
    pub port: String,
    pub was_connected_to: Endpoint,
}

pub async fn connect<P: PacketTracer>(
    packet_tracer: &P,
    request: &ConnectRequest,
) -> Result<Link, PtError> {
    let a = endpoint(&request.device_a, &request.port_a)?;
    let b = endpoint(&request.device_b, &request.port_b)?;
    if a == b {
        return Err(PtError::InvalidInput(
            "a port cannot be linked to itself".into(),
        ));
    }

    let (device_a, device_b) = tokio::try_join!(
        describe(packet_tracer, &a.device),
        describe(packet_tracer, &b.device),
    )?;
    for end in [&a, &b] {
        ensure_free(packet_tracer, end).await?;
    }

    let cable = request
        .cable
        .resolve((&device_a.kind, &a.port), (&device_b.kind, &b.port));
    let code = cable.code().ok_or_else(|| {
        PtError::InvalidInput("could not pick a cable for these ports; pass `cable`".into())
    })?;
    let created = packet_tracer
        .call(logical_workspace().method(
            "createLink",
            [
                Value::qstring(&a.device),
                Value::string(&a.port),
                Value::qstring(&b.device),
                Value::string(&b.port),
                Value::Int(code),
            ],
        ))
        .await?;
    if !expect_bool(&created, "createLink result")? {
        return Err(PtError::Rejected(format!(
            "no {} cable fits between {}:{} and {}:{}; the ports may not support that \
             medium, try another cable",
            cable.name(),
            a.device,
            a.port,
            b.device,
            b.port
        )));
    }

    let Some(Connection { to, cable }) = connection(packet_tracer, &a.device, &a.port).await?
    else {
        return Err(PtError::UnexpectedReply(
            "the new link is not visible on the port".into(),
        ));
    };
    Ok(Link { a, b: to, cable })
}

pub async fn disconnect<P: PacketTracer>(
    packet_tracer: &P,
    request: &PortRef,
) -> Result<Disconnected, PtError> {
    let end = endpoint(&request.device, &request.port)?;
    let Some(existing) = connection(packet_tracer, &end.device, &end.port).await? else {
        return Err(PtError::InvalidInput(format!(
            "{}:{} is not connected",
            end.device, end.port
        )));
    };
    let deleted = packet_tracer
        .call(logical_workspace().method(
            "deleteLink",
            [Value::qstring(&end.device), Value::string(&end.port)],
        ))
        .await?;
    if !expect_bool(&deleted, "deleteLink result")? {
        return Err(PtError::Rejected(format!(
            "the link on {}:{} was not removed",
            end.device, end.port
        )));
    }
    Ok(Disconnected {
        device: end.device,
        port: end.port,
        was_connected_to: existing.to,
    })
}

async fn ensure_free<P: PacketTracer>(packet_tracer: &P, end: &Endpoint) -> Result<(), PtError> {
    match connection(packet_tracer, &end.device, &end.port).await {
        Ok(None) => Ok(()),
        Ok(Some(existing)) => Err(PtError::InvalidInput(format!(
            "{}:{} is already connected to {}:{}",
            end.device, end.port, existing.to.device, existing.to.port
        ))),
        Err(PtError::NotFound(_)) => {
            let ports = port_names(packet_tracer, &end.device).await?;
            Err(PtError::InvalidInput(format!(
                "`{}` has no port `{}`; its ports are {}",
                end.device,
                end.port,
                ports.join(", ")
            )))
        }
        Err(other) => Err(other),
    }
}

fn endpoint(device: &str, port: &str) -> Result<Endpoint, PtError> {
    let (device, port) = (device.trim(), port.trim());
    if device.is_empty() || port.is_empty() {
        return Err(PtError::InvalidInput(
            "device and port names are required".into(),
        ));
    }
    Ok(Endpoint {
        device: device.to_owned(),
        port: port.to_owned(),
    })
}
