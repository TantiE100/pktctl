mod events;

use ptmp::Value;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use events::{EventList, EventsRequest, SimulationEvent, list_events};

use crate::{
    features::paths::app_window,
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_integer},
    server::PktctlServer,
};

use events::simulation;

const MAX_STEPS: u32 = 200;
const PDU_ERRORS: &[(i64, &str)] = &[
    (10, "Packet Tracer is not ready to add a PDU"),
    (20, "the source device does not exist"),
    (21, "the source device has no IP address"),
    (25, "the selected device has no IP address"),
    (30, "the destination device does not exist"),
    (31, "the destination device has no IP address"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ModeRequest {
    /// `true` for Simulation mode, `false` for Realtime.
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SimulationState {
    pub simulation: bool,
    /// Simulation clock in milliseconds.
    pub time: i64,
    /// Events recorded so far.
    pub events: i64,
    /// Index of the event the simulation is at; `back` moves it without deleting events.
    pub current_event: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    #[default]
    Forward,
    Back,
    Reset,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct StepRequest {
    /// `forward` (Capture/Forward), `back`, or `reset` (clears the event list).
    #[serde(default)]
    pub action: StepAction,
    /// How many steps for `forward` and `back`. Defaults to 1, maximum 200.
    #[serde(default)]
    pub times: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PduRequest {
    /// Device the ping starts from, for example `PC1`.
    pub source: String,
    /// Device it is sent to.
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PduAdded {
    pub source: String,
    pub destination: String,
    /// `realtime`: sent at once. `simulation`: waits for `simulation_step`.
    pub mode: String,
}

pub async fn set_mode<P: PacketTracer>(
    packet_tracer: &P,
    request: &ModeRequest,
) -> Result<SimulationState, PtError> {
    packet_tracer
        .call(simulation().method("setSimulationMode", [Value::Bool(request.on)]))
        .await?;
    state(packet_tracer).await
}

pub async fn state<P: PacketTracer>(packet_tracer: &P) -> Result<SimulationState, PtError> {
    let (mode, time, events, current) = tokio::try_join!(
        packet_tracer.call(simulation().method("isSimulationMode", [])),
        packet_tracer.call(simulation().method("getCurrentSimTime", [])),
        packet_tracer.call(simulation().method("getFrameInstanceCount", [])),
        packet_tracer.call(simulation().method("getCurrentFrameInstanceIndex", [])),
    )?;
    Ok(SimulationState {
        simulation: expect_bool(&mode, "isSimulationMode")?,
        time: expect_integer(&time, "simulation time")?,
        events: expect_integer(&events, "event count")?,
        current_event: expect_integer(&current, "current event")?,
    })
}

pub async fn step<P: PacketTracer>(
    packet_tracer: &P,
    request: &StepRequest,
) -> Result<SimulationState, PtError> {
    let times = request.times.unwrap_or(1);
    if times == 0 || times > MAX_STEPS {
        return Err(PtError::InvalidInput(format!(
            "times must be between 1 and {MAX_STEPS}"
        )));
    }
    let current = state(packet_tracer).await?;
    if !current.simulation {
        return Err(PtError::InvalidInput(
            "Packet Tracer is in Realtime mode; call simulation_mode with on: true first".into(),
        ));
    }
    let (method, repeat) = match request.action {
        StepAction::Forward => ("forward", times),
        StepAction::Back => ("backward", times),
        StepAction::Reset => ("resetSimulation", 1),
    };
    for _ in 0..repeat {
        packet_tracer.call(simulation().method(method, [])).await?;
    }
    state(packet_tracer).await
}

pub async fn add_pdu<P: PacketTracer>(
    packet_tracer: &P,
    request: &PduRequest,
) -> Result<PduAdded, PtError> {
    let (source, destination) = (request.source.trim(), request.destination.trim());
    if source.is_empty() || destination.is_empty() {
        return Err(PtError::InvalidInput(
            "source and destination are required".into(),
        ));
    }
    let code = packet_tracer
        .call(app_window().method("getUserCreatedPDU", []).method(
            "addSimplePdu",
            [Value::qstring(source), Value::qstring(destination)],
        ))
        .await?;
    let code = expect_integer(&code, "addSimplePdu result")?;
    if code != 0 {
        let reason = PDU_ERRORS
            .iter()
            .find(|(value, _)| *value == code)
            .map_or("Packet Tracer refused the PDU", |(_, reason)| reason);
        return Err(PtError::InvalidInput(format!(
            "cannot send from `{source}` to `{destination}`: {reason}"
        )));
    }
    let simulation = state(packet_tracer).await?.simulation;
    Ok(PduAdded {
        source: source.to_owned(),
        destination: destination.to_owned(),
        mode: if simulation { "simulation" } else { "realtime" }.into(),
    })
}

#[tool_router(router = simulation_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "simulation_mode",
        description = "Switch between Realtime and Simulation mode. Returns the mode, the \
                       simulation clock and how many events are recorded.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn simulation_mode_tool(
        &self,
        Parameters(request): Parameters<ModeRequest>,
    ) -> Result<Json<SimulationState>, String> {
        set_mode(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_pdu",
        description = "Send a simple PDU (ICMP echo) from one device to another, like the Add \
                       Simple PDU button. In Realtime mode it goes at once; in Simulation mode \
                       it waits for simulation_step.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_pdu_tool(
        &self,
        Parameters(request): Parameters<PduRequest>,
    ) -> Result<Json<PduAdded>, String> {
        add_pdu(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "simulation_step",
        description = "Advance the simulation (`forward`, like Capture/Forward), go `back`, or \
                       `reset` the event list. Needs Simulation mode.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn simulation_step_tool(
        &self,
        Parameters(request): Parameters<StepRequest>,
    ) -> Result<Json<SimulationState>, String> {
        step(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "list_simulation_events",
        description = "Read the simulation event list: where each PDU was, when, which \
                       protocol, and whether it was accepted or dropped. With \
                       include_decisions, also Packet Tracer's per-layer explanation of each \
                       step, the same text as the PDU Details window.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_simulation_events_tool(
        &self,
        Parameters(request): Parameters<EventsRequest>,
    ) -> Result<Json<EventList>, String> {
        list_events(self.packet_tracer(), &request)
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
        for name in ["PC1", "PC2"] {
            let request = AddDeviceRequest {
                model: "PC-PT".into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        packet_tracer
    }

    fn pdu(source: &str, destination: &str) -> PduRequest {
        PduRequest {
            source: source.into(),
            destination: destination.into(),
        }
    }

    #[tokio::test]
    async fn follows_a_ping_through_simulation_mode() {
        let packet_tracer = lab().await;
        let state = set_mode(&packet_tracer, &ModeRequest { on: true })
            .await
            .unwrap();
        assert!(state.simulation);

        let added = add_pdu(&packet_tracer, &pdu("PC1", "PC2")).await.unwrap();
        assert_eq!(added.mode, "simulation");
        let stepped = step(&packet_tracer, &StepRequest::default()).await.unwrap();
        assert_eq!((stepped.events, stepped.current_event), (2, 1));
        let back = step(
            &packet_tracer,
            &StepRequest {
                action: StepAction::Back,
                times: None,
            },
        )
        .await
        .unwrap();
        assert_eq!((back.events, back.current_event), (2, 0));

        let list = list_events(
            &packet_tracer,
            &EventsRequest {
                protocols: vec!["icmp".into()],
                include_decisions: true,
                ..EventsRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(list.total, 2);
        let arrived = &list.events[1];
        assert_eq!(
            (arrived.device.as_str(), arrived.from.as_deref()),
            ("PC2", Some("PC1"))
        );
        assert_eq!(arrived.protocol, "ICMP");
        assert_eq!(arrived.status, ["accepted"]);
        assert_eq!(arrived.decisions[0], "FastEthernet0 receives the frame.");
        assert!(list.events[0].from.is_none());

        let only_pc1 = list_events(
            &packet_tracer,
            &EventsRequest {
                device: Some("pc1".into()),
                limit: Some(1),
                ..EventsRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!((only_pc1.matching, only_pc1.events.len()), (1, 1));

        let reset = step(
            &packet_tracer,
            &StepRequest {
                action: StepAction::Reset,
                times: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(reset.events, 0);
    }

    #[tokio::test]
    async fn explains_refused_pdus_and_realtime_steps() {
        let packet_tracer = lab().await;
        let missing = add_pdu(&packet_tracer, &pdu("PC1", "PC9"))
            .await
            .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("destination device does not exist")
        );
        let realtime = add_pdu(&packet_tracer, &pdu("PC1", "PC2")).await.unwrap();
        assert_eq!(realtime.mode, "realtime");
        let refused = step(&packet_tracer, &StepRequest::default())
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("Realtime mode"));
        set_mode(&packet_tracer, &ModeRequest { on: true })
            .await
            .unwrap();
        let too_many = step(
            &packet_tracer,
            &StepRequest {
                times: Some(500),
                ..StepRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(too_many, PtError::InvalidInput(_)));
    }
}
