use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::list::{Device, count, describe};
use crate::{
    features::{
        catalog::find_device_model,
        paths::{device, logical_workspace},
    },
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_text},
};

const GRID_COLUMNS: i32 = 8;
const GRID_STEP: i32 = 120;
const GRID_ORIGIN: i32 = 100;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AddDeviceRequest {
    /// Model name as listed by `list_models`, for example `2911`, `2960-24TT`, `PC-PT`.
    pub model: String,
    /// Name to give the device. Packet Tracer picks one (Router0, PC1, ...) when omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// Canvas x of the device center. Give both x and y, or neither to use the next free grid slot.
    #[serde(default)]
    pub x: Option<i32>,
    /// Canvas y of the device center.
    #[serde(default)]
    pub y: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DeviceRef {
    /// Device name as shown by `list_devices`.
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RenameRequest {
    /// Current device name.
    pub name: String,
    /// New, unused device name.
    pub new_name: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MoveRequest {
    /// Device name.
    pub name: String,
    /// New canvas x of the device center.
    pub x: i32,
    /// New canvas y of the device center.
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Removed {
    pub removed: String,
}

pub async fn add<P: PacketTracer>(
    packet_tracer: &P,
    request: &AddDeviceRequest,
) -> Result<Device, PtError> {
    let wanted_name = request.name.as_deref().map(str::trim);
    if let Some(name) = wanted_name {
        require_name(name)?;
        ensure_unused(packet_tracer, name).await?;
    }
    let model = find_device_model(packet_tracer, &request.model).await?;
    let (x, y) = match (request.x, request.y) {
        (Some(x), Some(y)) => (x, y),
        (None, None) => next_grid_slot(count(packet_tracer).await?),
        _ => {
            return Err(PtError::InvalidInput(
                "give both x and y, or neither".into(),
            ));
        }
    };

    let created = packet_tracer
        .call(logical_workspace().method(
            "addDevice",
            [
                Value::Int(model.type_code),
                Value::string(&model.model),
                Value::Double(f64::from(x)),
                Value::Double(f64::from(y)),
            ],
        ))
        .await?;
    let created = expect_text(&created, "created device name")?;
    if created.is_empty() {
        return Err(PtError::Rejected(format!(
            "could not create a {}",
            model.model
        )));
    }

    if let Err(error) = packet_tracer
        .call(device(&created).method("skipBoot", []))
        .await
    {
        tracing::debug!(%error, device = created, "device has no boot sequence to skip");
    }

    match wanted_name {
        Some(name) if name != created => set_name(packet_tracer, &created, name).await,
        _ => describe(packet_tracer, &created).await,
    }
}

pub async fn remove<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<Removed, PtError> {
    let name = name.trim();
    describe(packet_tracer, name).await?;
    let removed = packet_tracer
        .call(logical_workspace().method("removeDevice", [Value::qstring(name)]))
        .await?;
    if !expect_bool(&removed, "removeDevice result")? {
        return Err(PtError::Rejected(format!(
            "device `{name}` was not removed"
        )));
    }
    Ok(Removed {
        removed: name.to_owned(),
    })
}

pub async fn rename<P: PacketTracer>(
    packet_tracer: &P,
    request: &RenameRequest,
) -> Result<Device, PtError> {
    let name = request.name.trim();
    let new_name = request.new_name.trim();
    require_name(new_name)?;
    describe(packet_tracer, name).await?;
    if name == new_name {
        return describe(packet_tracer, name).await;
    }
    ensure_unused(packet_tracer, new_name).await?;
    set_name(packet_tracer, name, new_name).await
}

pub async fn relocate<P: PacketTracer>(
    packet_tracer: &P,
    request: &MoveRequest,
) -> Result<Device, PtError> {
    let name = request.name.trim();
    describe(packet_tracer, name).await?;
    let moved = packet_tracer
        .call(device(name).method(
            "moveToLocationCentered",
            [Value::Int(request.x), Value::Int(request.y)],
        ))
        .await?;
    if !expect_bool(&moved, "moveToLocationCentered result")? {
        return Err(PtError::Rejected(format!("device `{name}` was not moved")));
    }
    describe(packet_tracer, name).await
}

async fn set_name<P: PacketTracer>(
    packet_tracer: &P,
    current: &str,
    new_name: &str,
) -> Result<Device, PtError> {
    packet_tracer
        .call(device(current).method("setName", [Value::qstring(new_name)]))
        .await?;
    describe(packet_tracer, new_name).await
}

async fn ensure_unused<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<(), PtError> {
    match describe(packet_tracer, name).await {
        Ok(_) => Err(PtError::InvalidInput(format!(
            "a device named `{name}` already exists"
        ))),
        Err(PtError::NotFound(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

fn require_name(name: &str) -> Result<(), PtError> {
    if name.is_empty() {
        return Err(PtError::InvalidInput("device names cannot be blank".into()));
    }
    Ok(())
}

fn next_grid_slot(existing: i32) -> (i32, i32) {
    (
        GRID_ORIGIN + (existing % GRID_COLUMNS) * GRID_STEP,
        GRID_ORIGIN + (existing / GRID_COLUMNS) * GRID_STEP,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fills_rows_of_eight() {
        assert_eq!(next_grid_slot(0), (100, 100));
        assert_eq!(next_grid_slot(7), (940, 100));
        assert_eq!(next_grid_slot(8), (100, 220));
    }
}
