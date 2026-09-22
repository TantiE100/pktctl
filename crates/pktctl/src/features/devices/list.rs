use futures::future::try_join_all;
use ptmp::Call;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    features::paths::{device, device_at, network},
    packet_tracer::{
        PacketTracer, PtError, expect_integer, expect_number, expect_text, kinds::device_kind,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Device {
    pub name: String,
    pub model: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct DeviceList {
    pub devices: Vec<Device>,
}

pub async fn count<P: PacketTracer>(packet_tracer: &P) -> Result<i32, PtError> {
    let count = packet_tracer
        .call(network().method("getDeviceCount", []))
        .await?;
    let count = expect_integer(&count, "device count")?;
    i32::try_from(count)
        .map_err(|_| PtError::UnexpectedReply(format!("device count {count} is out of range")))
}

pub async fn list<P: PacketTracer>(packet_tracer: &P) -> Result<DeviceList, PtError> {
    let count = count(packet_tracer).await?;
    let devices =
        try_join_all((0..count).map(|index| read(packet_tracer, device_at(index)))).await?;
    Ok(DeviceList { devices })
}

pub async fn describe<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<Device, PtError> {
    read(packet_tracer, device(name))
        .await
        .map_err(|error| match error {
            PtError::NotFound(_) => PtError::NotFound(format!("device `{name}`")),
            other => other,
        })
}

async fn read<P: PacketTracer>(packet_tracer: &P, selector: Call) -> Result<Device, PtError> {
    let get = |method: &'static str| packet_tracer.call(selector.clone().method(method, []));
    let (name, model, kind, x, y) = tokio::try_join!(
        get("getName"),
        get("getModel"),
        get("getType"),
        get("getCenterXCoordinate"),
        get("getCenterYCoordinate"),
    )?;
    Ok(Device {
        name: expect_text(&name, "device name")?,
        model: expect_text(&model, "device model")?,
        kind: device_kind(expect_integer(&kind, "device type")?),
        x: expect_number(&x, "x coordinate")?,
        y: expect_number(&y, "y coordinate")?,
    })
}
