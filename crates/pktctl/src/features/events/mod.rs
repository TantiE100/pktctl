use std::time::Duration;

use ptmp::{Event, Subscription};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json_;
use tokio::{sync::broadcast::error::RecvError, time::Instant};

use crate::{
    features::ipc::plain,
    packet_tracer::{PacketTracer, PtError, api::ApiIndex},
    server::PktctlServer,
};

const ALL_OBJECTS: &str = "";
const DEFAULT_SECONDS: u64 = 10;
const MAX_SECONDS: u64 = 120;
const DEFAULT_MAX_EVENTS: usize = 100;
const MAX_EVENTS: usize = 1000;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WatchRequest {
    /// Object class raising the events, for example `LogicalWorkspace`, `TerminalLine` or
    /// `Simulation`. `describe_ipc` with `events` lists them all.
    pub class: String,
    /// Event names, for example `["deviceAdded", "linkCreated"]`. Omit for every event of the class.
    #[serde(default)]
    pub events: Vec<String>,
    /// Only events from this object, by uuid (as returned by `call_ipc`). Omit for every object.
    #[serde(default)]
    pub object: Option<String>,
    /// How long to listen. Defaults to 10 seconds, maximum 120.
    #[serde(default)]
    pub seconds: Option<u64>,
    /// Stop after this many events. Defaults to 100, maximum 1000.
    #[serde(default)]
    pub max_events: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct WatchedEvent {
    pub class: String,
    pub object: String,
    pub event: String,
    pub args: Vec<Json_>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct WatchResult {
    pub events: Vec<WatchedEvent>,
    /// True when `max_events` stopped the watch before `seconds` elapsed.
    pub truncated: bool,
}

pub async fn watch<P: PacketTracer>(
    packet_tracer: &P,
    request: &WatchRequest,
) -> Result<WatchResult, PtError> {
    let api = ApiIndex::get();
    let (class, known) = api.event_class(request.class.trim()).ok_or_else(|| {
        PtError::InvalidInput(format!(
            "`{}` raises no events; call describe_ipc with {{\"events\": \"\"}} for the classes",
            request.class
        ))
    })?;
    let names: Vec<String> = if request.events.is_empty() {
        known.to_vec()
    } else {
        request
            .events
            .iter()
            .map(|wanted| {
                known
                    .iter()
                    .find(|event| event.eq_ignore_ascii_case(wanted.trim()))
                    .cloned()
                    .ok_or_else(|| {
                        PtError::InvalidInput(format!(
                            "`{class}` has no event `{wanted}`; it has {}",
                            known.join(", ")
                        ))
                    })
            })
            .collect::<Result<_, _>>()?
    };
    let seconds = request.seconds.unwrap_or(DEFAULT_SECONDS);
    if seconds == 0 || seconds > MAX_SECONDS {
        return Err(PtError::InvalidInput(format!(
            "seconds must be between 1 and {MAX_SECONDS}"
        )));
    }
    let limit = request
        .max_events
        .unwrap_or(DEFAULT_MAX_EVENTS)
        .clamp(1, MAX_EVENTS);
    let object = request
        .object
        .as_deref()
        .map(str::trim)
        .filter(|object| !object.is_empty())
        .unwrap_or(ALL_OBJECTS)
        .to_owned();

    let subscriptions: Vec<Subscription> = names
        .iter()
        .map(|event| Subscription::to(class, &object, event))
        .collect();
    let mut receiver = packet_tracer.subscribe(subscriptions[0].clone()).await?;
    for subscription in &subscriptions[1..] {
        packet_tracer.subscribe(subscription.clone()).await?;
    }

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut events = Vec::new();
    let mut truncated = false;
    loop {
        let event: Event = match tokio::time::timeout_at(deadline, receiver.recv()).await {
            Err(_) | Ok(Err(RecvError::Closed)) => break,
            Ok(Err(RecvError::Lagged(missed))) => {
                tracing::warn!(missed, "events were dropped while watching");
                continue;
            }
            Ok(Ok(event)) => event,
        };
        let matches = event.class == class
            && names.contains(&event.name)
            && (object.is_empty() || event.object_uuid.eq_ignore_ascii_case(&object));
        if !matches {
            continue;
        }
        events.push(WatchedEvent {
            class: event.class,
            object: event.object_uuid,
            event: event.name,
            args: event.args.into_iter().map(|arg| plain(api, arg)).collect(),
        });
        if events.len() >= limit {
            truncated = true;
            break;
        }
    }

    for subscription in subscriptions {
        if let Err(error) = packet_tracer.unsubscribe(subscription).await {
            tracing::debug!(%error, "could not unsubscribe");
        }
    }
    Ok(WatchResult { events, truncated })
}

#[tool_router(router = events_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "watch_events",
        description = "Listen to Packet Tracer's live IPC events for a few seconds and return \
                       them: devices and links added or removed, console output, simulation \
                       steps, ARP and DHCP activity, and 200 more across 73 classes. Start \
                       something (for example with another tool call in parallel, or by hand in \
                       Packet Tracer) and see what it triggers. describe_ipc with `events` lists \
                       the classes and names.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn watch_events_tool(
        &self,
        Parameters(request): Parameters<WatchRequest>,
    ) -> Result<Json<WatchResult>, String> {
        watch(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use ptmp::Value;

    use super::*;
    use crate::packet_tracer::scripted::{ScriptedPacketTracer, methods};

    fn device_added(name: &str) -> Event {
        Event {
            token: "1".into(),
            class: "LogicalWorkspace".into(),
            object_uuid: "{lw}".into(),
            name: "deviceAdded".into(),
            args: vec![
                Value::qstring(name),
                Value::string("2911"),
                Value::Uuid("{r1}".into()),
            ],
        }
    }

    #[tokio::test(start_paused = true)]
    async fn collects_matching_events_until_the_limit() {
        let packet_tracer = ScriptedPacketTracer::with_events(|_, _| Ok(Value::Void));
        let request = WatchRequest {
            class: "logicalworkspace".into(),
            events: vec!["DEVICEADDED".into()],
            max_events: Some(2),
            ..WatchRequest::default()
        };
        let watcher = watch(&packet_tracer, &request);
        let emitter = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            packet_tracer.emit(Event {
                name: "linkCreated".into(),
                ..device_added("ignored")
            });
            for name in ["R1", "R2", "R3"] {
                packet_tracer.emit(device_added(name));
            }
        };
        let (result, ()) = tokio::join!(watcher, emitter);
        let result = result.unwrap();
        assert!(result.truncated);
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.events[0].args[0], serde_json::json!("R1"));
        let subscriptions = packet_tracer.subscriptions();
        assert_eq!(subscriptions[0].object_uuid, "");
        assert!(
            subscriptions
                .iter()
                .any(|subscription| !subscription.enabled)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn returns_after_the_time_window() {
        let packet_tracer =
            ScriptedPacketTracer::new(|call| panic!("unexpected {:?}", methods(call)));
        let result = watch(
            &packet_tracer,
            &WatchRequest {
                class: "TerminalLine".into(),
                seconds: Some(3),
                ..WatchRequest::default()
            },
        )
        .await
        .unwrap();
        assert!(result.events.is_empty());
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn validates_classes_and_event_names() {
        let packet_tracer = ScriptedPacketTracer::new(|_| Ok(Value::Void));
        let unknown = watch(
            &packet_tracer,
            &WatchRequest {
                class: "Router".into(),
                ..WatchRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(unknown.to_string().contains("raises no events"));
        let bad_event = watch(
            &packet_tracer,
            &WatchRequest {
                class: "LogicalWorkspace".into(),
                events: vec!["deviceExploded".into()],
                ..WatchRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(bad_event.to_string().contains("deviceAdded"));
    }
}
