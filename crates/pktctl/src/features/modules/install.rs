use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::slots::{Slot, list_slots};
use crate::{
    features::{
        catalog::find_module_model,
        devices::ready_console,
        links::{Endpoint, Link, list_ports},
        paths::device,
    },
    packet_tracer::{PacketTracer, PtError, expect_bool},
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AddModuleRequest {
    /// Router or switch name.
    pub device: String,
    /// Empty slot path from `list_slots`, for example `0/1`.
    pub slot: String,
    /// Module model supported by the device, for example `HWIC-2T` or `NIM-2T`.
    pub module: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RemoveModuleRequest {
    /// Router or switch name.
    pub device: String,
    /// Slot path holding the module to remove.
    pub slot: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ModuleChange {
    pub device: String,
    pub slot: String,
    pub module: String,
    pub ports_added: Vec<String>,
    pub ports_removed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut_links: Vec<Link>,
}

pub async fn add_module<P: PacketTracer>(
    packet_tracer: &P,
    request: &AddModuleRequest,
) -> Result<ModuleChange, PtError> {
    let (device_name, slot_path) = (request.device.trim(), request.slot.trim());
    let inventory = list_slots(packet_tracer, device_name).await?;
    let supported = inventory
        .supported_modules
        .iter()
        .find(|model| model.eq_ignore_ascii_case(request.module.trim()))
        .ok_or_else(|| {
            PtError::InvalidInput(format!(
                "`{device_name}` does not take `{}`; it supports {}",
                request.module.trim(),
                list_or_none(&inventory.supported_modules)
            ))
        })?;
    let module = find_module_model(packet_tracer, supported).await?;

    let slot = find_slot(&inventory.slots, slot_path, device_name)?;
    if let Some(installed) = &slot.installed {
        return Err(PtError::InvalidInput(format!(
            "slot {slot_path} on `{device_name}` already holds {installed}; remove it first"
        )));
    }
    if slot.accepts_code != i64::from(module.type_code) {
        let fitting: Vec<_> = inventory
            .slots
            .iter()
            .filter(|slot| {
                slot.installed.is_none() && slot.accepts_code == i64::from(module.type_code)
            })
            .map(|slot| slot.path.as_str())
            .collect();
        return Err(PtError::InvalidInput(format!(
            "slot {slot_path} accepts {}, not {} like {}; free slots that fit: {}",
            slot.accepts,
            module.kind,
            module.model,
            list_or_none(&fitting)
        )));
    }

    let installed = with_power_off(packet_tracer, device_name, || async {
        packet_tracer
            .call(device(device_name).method(
                "addModule",
                [
                    Value::string(slot_path),
                    Value::Int(module.type_code),
                    Value::string(&module.model),
                ],
            ))
            .await
    })
    .await?;
    if !installed.applied {
        return Err(PtError::Rejected(format!(
            "{} was not installed in slot {slot_path} of `{device_name}`",
            module.model
        )));
    }
    Ok(ModuleChange {
        device: device_name.to_owned(),
        slot: slot_path.to_owned(),
        module: module.model,
        ports_added: installed.added,
        ports_removed: installed.removed,
        cut_links: installed.cut,
    })
}

pub async fn remove_module<P: PacketTracer>(
    packet_tracer: &P,
    request: &RemoveModuleRequest,
) -> Result<ModuleChange, PtError> {
    let (device_name, slot_path) = (request.device.trim(), request.slot.trim());
    let inventory = list_slots(packet_tracer, device_name).await?;
    let slot = find_slot(&inventory.slots, slot_path, device_name)?;
    let Some(module) = slot.installed.clone() else {
        return Err(PtError::InvalidInput(format!(
            "slot {slot_path} on `{device_name}` is empty"
        )));
    };

    let removed = with_power_off(packet_tracer, device_name, || async {
        packet_tracer
            .call(device(device_name).method("removeModule", [Value::string(slot_path)]))
            .await
    })
    .await?;
    if !removed.applied {
        return Err(PtError::Rejected(format!(
            "{module} was not removed from slot {slot_path} of `{device_name}`"
        )));
    }
    Ok(ModuleChange {
        device: device_name.to_owned(),
        slot: slot_path.to_owned(),
        module,
        ports_added: removed.added,
        ports_removed: removed.removed,
        cut_links: removed.cut,
    })
}

async fn with_power_off<P, F, Fut>(
    packet_tracer: &P,
    device_name: &str,
    change: F,
) -> Result<PortDelta, PtError>
where
    P: PacketTracer,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Value, PtError>>,
{
    let before = list_ports(packet_tracer, device_name).await?.ports;
    set_power(packet_tracer, device_name, false).await?;
    let outcome = change().await;
    set_power(packet_tracer, device_name, true).await?;
    if let Err(error) = packet_tracer
        .call(device(device_name).method("skipBoot", []))
        .await
    {
        tracing::debug!(%error, "device has no boot sequence to skip");
    }
    if let Err(error) = ready_console(packet_tracer, device_name).await {
        tracing::warn!(%error, device = device_name, "console left at its first prompt");
    }
    let applied = expect_bool(&outcome?, "module change result")?;

    let after = port_names(packet_tracer, device_name).await?;
    let known: Vec<&str> = before.iter().map(|port| port.name.as_str()).collect();
    let added = after
        .iter()
        .filter(|port| !known.contains(&port.as_str()))
        .cloned()
        .collect();
    let gone: Vec<_> = before
        .into_iter()
        .filter(|port| !after.contains(&port.name))
        .collect();
    let cut = gone
        .iter()
        .filter_map(|port| {
            port.connection.as_ref().map(|connection| Link {
                a: Endpoint {
                    device: device_name.to_owned(),
                    port: port.name.clone(),
                },
                b: connection.to.clone(),
                cable: connection.cable.clone(),
            })
        })
        .collect();
    Ok(PortDelta {
        applied,
        added,
        removed: gone.into_iter().map(|port| port.name).collect(),
        cut,
    })
}

struct PortDelta {
    applied: bool,
    added: Vec<String>,
    removed: Vec<String>,
    cut: Vec<Link>,
}

async fn set_power<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    on: bool,
) -> Result<(), PtError> {
    packet_tracer
        .call(device(device_name).method("setPower", [Value::Bool(on)]))
        .await
        .map(drop)
}

async fn port_names<P: PacketTracer>(
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

fn find_slot<'a>(slots: &'a [Slot], path: &str, device_name: &str) -> Result<&'a Slot, PtError> {
    slots.iter().find(|slot| slot.path == path).ok_or_else(|| {
        let paths: Vec<_> = slots.iter().map(|slot| slot.path.as_str()).collect();
        PtError::InvalidInput(format!(
            "`{device_name}` has no module slot {path}; its slots are {}",
            list_or_none(&paths)
        ))
    })
}

fn list_or_none<S: AsRef<str>>(items: &[S]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
