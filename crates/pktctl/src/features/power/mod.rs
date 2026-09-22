use ptmp::Value;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{
        devices::{describe, list, ready_console},
        paths::{app_window, device},
    },
    packet_tracer::{PacketTracer, PtError, expect_bool, kinds::runs_ios},
    server::PktctlServer,
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PowerRequest {
    /// Device name as returned by `list_devices`.
    pub device: String,
    /// `true` to switch it on, `false` to switch it off.
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PowerState {
    pub device: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PowerCycled {
    /// Devices that were on and have been switched off and back on.
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Done {
    pub done: bool,
}

pub async fn set_power<P: PacketTracer>(
    packet_tracer: &P,
    request: &PowerRequest,
) -> Result<PowerState, PtError> {
    let name = request.device.trim();
    let target = describe(packet_tracer, name).await?;
    let was_on = packet_tracer
        .call(device(name).method("getPower", []))
        .await?;
    let was_on = expect_bool(&was_on, "getPower")?;
    if was_on != request.on {
        packet_tracer
            .call(device(name).method("setPower", [Value::Bool(request.on)]))
            .await?;
        if request.on && runs_ios(&target.kind) {
            packet_tracer
                .call(device(name).method("skipBoot", []))
                .await?;
            ready_console(packet_tracer, name).await?;
        }
    }
    let on = packet_tracer
        .call(device(name).method("getPower", []))
        .await?;
    let on = expect_bool(&on, "getPower")?;
    if on != request.on {
        return Err(PtError::Rejected(format!(
            "`{name}` is still {}",
            if on { "on" } else { "off" }
        )));
    }
    Ok(PowerState {
        device: name.to_owned(),
        on,
    })
}

pub async fn fast_forward<P: PacketTracer>(packet_tracer: &P) -> Result<Done, PtError> {
    packet_tracer
        .call(
            app_window()
                .method("getRealtimeToolbar", [])
                .method("fastForwardTime", []),
        )
        .await?;
    Ok(Done { done: true })
}

/// Does what the Power Cycle Devices button does, one device at a time: the button
/// itself opens a confirmation dialog that blocks every IPC call until someone
/// answers it.
pub async fn power_cycle_all<P: PacketTracer>(packet_tracer: &P) -> Result<PowerCycled, PtError> {
    let mut cycled = Vec::new();
    for listed in list(packet_tracer).await?.devices {
        let powered = packet_tracer
            .call(device(&listed.name).method("getPower", []))
            .await?;
        if !expect_bool(&powered, "getPower")? {
            continue;
        }
        packet_tracer
            .call(device(&listed.name).method("setPower", [Value::Bool(false)]))
            .await?;
        cycled.push(listed.name);
    }
    for name in &cycled {
        set_power(
            packet_tracer,
            &PowerRequest {
                device: name.clone(),
                on: true,
            },
        )
        .await?;
    }
    Ok(PowerCycled { devices: cycled })
}

#[tool_router(router = power_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "set_power",
        description = "Switch a device on or off. Switching a router or switch off and on \
                       reloads it: configuration that was not saved with write memory is lost. \
                       IOS devices come back ready at the prompt.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn set_power_tool(
        &self,
        Parameters(request): Parameters<PowerRequest>,
    ) -> Result<Json<PowerState>, String> {
        set_power(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "fast_forward",
        description = "Press Realtime mode's Fast Forward Time button: timers jump ahead so \
                       spanning tree converges, DHCP leases arrive and routing protocols settle \
                       at once instead of after 30 seconds or more. Call it after cabling or \
                       configuring, before testing connectivity.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn fast_forward_tool(&self) -> Result<Json<Done>, String> {
        fast_forward(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "power_cycle_all",
        description = "Power cycle every device that is on, like the Power Cycle Devices button \
                       but without its confirmation dialog: every device reloads and unsaved \
                       configuration on every router and switch is lost. Returns the devices \
                       cycled.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn power_cycle_all_tool(&self) -> Result<Json<PowerCycled>, String> {
        power_cycle_all(self.packet_tracer())
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

    #[tokio::test]
    async fn switches_devices_off_and_back_on_ready_at_the_prompt() {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        let request = AddDeviceRequest {
            model: "2911".into(),
            name: Some("R1".into()),
            ..AddDeviceRequest::default()
        };
        add(&packet_tracer, &request).await.unwrap();

        let off = set_power(
            &packet_tracer,
            &PowerRequest {
                device: "R1".into(),
                on: false,
            },
        )
        .await
        .unwrap();
        assert!(!off.on);
        assert_eq!(canvas.is_powered("R1"), Some(false));

        let on = set_power(
            &packet_tracer,
            &PowerRequest {
                device: "R1".into(),
                on: true,
            },
        )
        .await
        .unwrap();
        assert!(on.on);
        assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router>"));

        let missing = set_power(
            &packet_tracer,
            &PowerRequest {
                device: "R9".into(),
                on: true,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(missing, PtError::NotFound(_)));
    }

    #[tokio::test]
    async fn fast_forward_presses_the_realtime_button() {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        assert!(fast_forward(&packet_tracer).await.unwrap().done);
        assert_eq!(canvas.realtime_presses(), ["fastForwardTime"]);
    }

    #[tokio::test]
    async fn power_cycle_all_reloads_what_is_on_without_the_dialog_button() {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        for (model, name) in [("2911", "R1"), ("2960-24TT", "S1"), ("PC-PT", "PC1")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        set_power(
            &packet_tracer,
            &PowerRequest {
                device: "PC1".into(),
                on: false,
            },
        )
        .await
        .unwrap();

        let cycled = power_cycle_all(&packet_tracer).await.unwrap();
        assert_eq!(cycled.devices, ["R1", "S1"]);
        assert_eq!(canvas.is_powered("R1"), Some(true));
        assert_eq!(canvas.is_powered("PC1"), Some(false));
        assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router>"));
        assert!(canvas.realtime_presses().is_empty());
    }
}
