use ptmp::Value;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{
        devices::{describe, ready_console},
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

pub async fn power_cycle_all<P: PacketTracer>(packet_tracer: &P) -> Result<Done, PtError> {
    packet_tracer
        .call(
            app_window()
                .method("getRealtimeToolbar", [])
                .method("resetNetwork", []),
        )
        .await?;
    Ok(Done { done: true })
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
        description = "Press the Power Cycle Devices button: every device reloads. Unsaved \
                       configuration on every router and switch is lost.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn power_cycle_all_tool(&self) -> Result<Json<Done>, String> {
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
    async fn presses_the_realtime_buttons() {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        assert!(fast_forward(&packet_tracer).await.unwrap().done);
        assert!(power_cycle_all(&packet_tracer).await.unwrap().done);
        assert_eq!(
            canvas.realtime_presses(),
            ["fastForwardTime", "resetNetwork"]
        );
    }
}
