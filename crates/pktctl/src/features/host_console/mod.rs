use std::time::Duration;

use ptmp::{Call, Event, Subscription, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{sync::broadcast::error::RecvError, time::Instant};

use crate::{
    packet_tracer::{CommandStatus, Events, PacketTracer, PtError, expect_integer, expect_text},
    server::PktctlServer,
};

const TERMINAL: &str = "TerminalLine";
const OUTPUT_WRITTEN: &str = "outputWritten";
const COMMAND_ENDED: &str = "commandEnded";
const MORE_DISPLAYED: &str = "moreDisplayed";
const SPACE: i8 = 32;
const NO_SPECIAL_CHAR: i32 = 0;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HostCommandRequest {
    /// PC, laptop or server name as returned by `list_devices`.
    pub device: String,
    /// Command Prompt command, for example `ping 192.168.1.1` or `ipconfig /all`.
    pub command: String,
    /// Seconds to wait for the command to finish. Defaults to 30, maximum 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HostCommandResult {
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<CommandStatus>,
    pub output: String,
}

pub async fn run<P: PacketTracer>(
    packet_tracer: &P,
    request: &HostCommandRequest,
) -> Result<HostCommandResult, PtError> {
    let device = request.device.trim();
    let command = request.command.trim();
    if device.is_empty() || command.is_empty() {
        return Err(PtError::InvalidInput(
            "device and command are required".into(),
        ));
    }
    let timeout_secs = request.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if timeout_secs == 0 || timeout_secs > MAX_TIMEOUT_SECS {
        return Err(PtError::InvalidInput(format!(
            "timeout_secs must be between 1 and {MAX_TIMEOUT_SECS}"
        )));
    }

    let prompt = Call::root("network")
        .method("getDevice", [Value::qstring(device)])
        .method("getCommandPrompt", []);
    let terminal = packet_tracer
        .call(prompt.clone().method("getObjectUuid", []))
        .await
        .map_err(explain_non_hosts)?;
    let terminal = expect_text(&terminal, "terminal id")?;

    let subscriptions = [OUTPUT_WRITTEN, COMMAND_ENDED, MORE_DISPLAYED]
        .map(|event| Subscription::to(TERMINAL, &terminal, event));
    let mut events = packet_tracer.subscribe(subscriptions[0].clone()).await?;
    for subscription in &subscriptions[1..] {
        packet_tracer.subscribe(subscription.clone()).await?;
    }

    let entered = packet_tracer
        .call(
            prompt
                .clone()
                .method("enterCommand", [Value::string(command)]),
        )
        .await;
    let outcome = match entered {
        Ok(_) => {
            let console = Console {
                packet_tracer,
                prompt: &prompt,
                terminal: &terminal,
            };
            console
                .collect(&mut events, Duration::from_secs(timeout_secs))
                .await
        }
        Err(error) => Err(error),
    };

    for subscription in subscriptions {
        if let Err(error) = packet_tracer.unsubscribe(subscription).await {
            tracing::debug!(%error, "could not unsubscribe from terminal events");
        }
    }
    outcome
}

struct Console<'a, P> {
    packet_tracer: &'a P,
    prompt: &'a Call,
    terminal: &'a str,
}

impl<P: PacketTracer> Console<'_, P> {
    async fn collect(
        &self,
        events: &mut Events,
        timeout: Duration,
    ) -> Result<HostCommandResult, PtError> {
        let deadline = Instant::now() + timeout;
        let mut output = String::new();
        loop {
            let event = match tokio::time::timeout_at(deadline, events.recv()).await {
                Err(_) => {
                    return Ok(HostCommandResult {
                        finished: false,
                        status: None,
                        output,
                    });
                }
                Ok(Err(RecvError::Lagged(missed))) => {
                    tracing::warn!(missed, "terminal output events were dropped");
                    continue;
                }
                Ok(Err(RecvError::Closed)) => {
                    return Err(PtError::Unreachable(
                        "connection closed while the command ran".into(),
                    ));
                }
                Ok(Ok(event)) => event,
            };
            if event.class != TERMINAL || event.object_uuid != self.terminal {
                continue;
            }
            match event.name.as_str() {
                OUTPUT_WRITTEN => output.push_str(first_text(&event)?),
                MORE_DISPLAYED => self.next_page().await?,
                COMMAND_ENDED => {
                    return Ok(HostCommandResult {
                        finished: true,
                        status: Some(ended_status(&event)?),
                        output,
                    });
                }
                _ => {}
            }
        }
    }

    async fn next_page(&self) -> Result<(), PtError> {
        self.packet_tracer
            .call(self.prompt.clone().method(
                "enterChar",
                [Value::Byte(SPACE), Value::Int(NO_SPECIAL_CHAR)],
            ))
            .await
            .map(drop)
    }
}

fn first_text(event: &Event) -> Result<&str, PtError> {
    event
        .args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| PtError::UnexpectedReply(format!("{} without text: {event:?}", event.name)))
}

fn ended_status(event: &Event) -> Result<CommandStatus, PtError> {
    let code = event.args.get(1).ok_or_else(|| {
        PtError::UnexpectedReply(format!("commandEnded without status: {event:?}"))
    })?;
    let code = expect_integer(code, "command status")?;
    CommandStatus::from_code(code)
        .ok_or_else(|| PtError::UnexpectedReply(format!("unknown command status {code}")))
}

fn explain_non_hosts(error: PtError) -> PtError {
    match error {
        PtError::Rejected(reason) if reason.contains("getCommandPrompt") => {
            PtError::Rejected(format!(
                "{reason}; this device has no Command Prompt, use run_cli for routers and switches"
            ))
        }
        other => other,
    }
}

#[tool_router(router = host_console_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "run_host_command",
        description = "Run a command in the Command Prompt of a PC, laptop or server \
                       (ping, ipconfig, tracert, nslookup, arp -a) and return its output. \
                       Waits until the command finishes or `timeout_secs` elapses; \
                       `finished: false` means it was still running.",
        annotations(read_only_hint = false, open_world_hint = false)
    )]
    async fn run_host_command_tool(
        &self,
        Parameters(request): Parameters<HostCommandRequest>,
    ) -> Result<Json<HostCommandResult>, String> {
        run(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_tracer::scripted::{Emitter, ScriptedPacketTracer, methods};

    const TERMINAL_ID: &str = "{terminal-pc1}";

    fn terminal_event(name: &str, args: Vec<Value>) -> Event {
        Event {
            token: "1".into(),
            class: TERMINAL.into(),
            object_uuid: TERMINAL_ID.into(),
            name: name.into(),
            args,
        }
    }

    fn written(text: &str) -> Event {
        terminal_event(
            OUTPUT_WRITTEN,
            vec![Value::string(text), Value::Bool(false), Value::Int(0)],
        )
    }

    fn ended(status: i32) -> Event {
        terminal_event(
            COMMAND_ENDED,
            vec![Value::string("ping 10.0.0.2"), Value::Int(status)],
        )
    }

    fn pc(on_command: impl Fn(&Emitter) + Send + Sync + 'static) -> ScriptedPacketTracer {
        ScriptedPacketTracer::with_events(move |call, emitter| match methods(call).as_slice() {
            [.., "getCommandPrompt", "getObjectUuid"] => Ok(Value::Uuid(TERMINAL_ID.into())),
            [.., "getCommandPrompt", "enterCommand"] => {
                on_command(emitter);
                Ok(Value::Void)
            }
            other => panic!("unexpected call {other:?}"),
        })
    }

    #[tokio::test]
    async fn pages_through_more_prompts_with_a_space() {
        let packet_tracer =
            ScriptedPacketTracer::with_events(|call, emitter| match methods(call).as_slice() {
                [.., "getObjectUuid"] => Ok(Value::Uuid(TERMINAL_ID.into())),
                [.., "enterCommand"] => {
                    emitter.emit(written("page one\n"));
                    emitter.emit(terminal_event(MORE_DISPLAYED, vec![Value::Int(0)]));
                    Ok(Value::Void)
                }
                [.., "enterChar"] => {
                    assert_eq!(call.steps()[3].args, [Value::Byte(32), Value::Int(0)]);
                    emitter.emit(written("page two\n"));
                    emitter.emit(ended(0));
                    Ok(Value::Void)
                }
                other => panic!("unexpected call {other:?}"),
            });
        let result = run(&packet_tracer, &request(None)).await.unwrap();
        assert!(result.finished);
        assert_eq!(result.output, "page one\npage two\n");
    }

    fn request(timeout_secs: Option<u64>) -> HostCommandRequest {
        HostCommandRequest {
            device: "PC1".into(),
            command: "ping 10.0.0.2".into(),
            timeout_secs,
        }
    }

    #[tokio::test]
    async fn gathers_output_until_the_command_ends() {
        let packet_tracer = pc(|emitter| {
            emitter.emit(written("Reply from 10.0.0.2: bytes=32 time<1ms TTL=128\n"));
            emitter.emit(written(
                "Packets: Sent = 1, Received = 1, Lost = 0 (0% loss)\n",
            ));
            emitter.emit(ended(0));
        });
        let result = run(&packet_tracer, &request(None)).await.unwrap();
        assert_eq!(
            result,
            HostCommandResult {
                finished: true,
                status: Some(CommandStatus::Ok),
                output: "Reply from 10.0.0.2: bytes=32 time<1ms TTL=128\nPackets: Sent = 1, Received = 1, Lost = 0 (0% loss)\n".into(),
            }
        );
    }

    #[tokio::test]
    async fn subscribes_before_typing_and_unsubscribes_after() {
        let packet_tracer = pc(|emitter| emitter.emit(ended(0)));
        run(&packet_tracer, &request(None)).await.unwrap();
        let subscriptions = packet_tracer.subscriptions();
        assert_eq!(subscriptions.len(), 6);
        assert!(
            subscriptions[..3]
                .iter()
                .all(|subscription| subscription.enabled)
        );
        assert!(
            subscriptions[3..]
                .iter()
                .all(|subscription| !subscription.enabled)
        );
        assert_eq!(subscriptions[0].object_uuid, TERMINAL_ID);
    }

    #[tokio::test]
    async fn ignores_other_terminals() {
        let packet_tracer = pc(|emitter| {
            let mut foreign = written("not mine\n");
            foreign.object_uuid = "{other}".into();
            emitter.emit(foreign);
            emitter.emit(written("mine\n"));
            emitter.emit(ended(0));
        });
        assert_eq!(
            run(&packet_tracer, &request(None)).await.unwrap().output,
            "mine\n"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn returns_partial_output_when_the_command_outlives_the_timeout() {
        let packet_tracer = pc(|emitter| emitter.emit(written("Tracing route...\n")));
        let result = run(&packet_tracer, &request(Some(5))).await.unwrap();
        assert!(!result.finished);
        assert_eq!(result.status, None);
        assert_eq!(result.output, "Tracing route...\n");
    }

    #[tokio::test]
    async fn explains_that_routers_have_no_command_prompt() {
        let packet_tracer = ScriptedPacketTracer::new(|_| {
            Err(PtError::Rejected(
                r#"Router: IPC call "getCommandPrompt" not found"#.into(),
            ))
        });
        let error = run(&packet_tracer, &request(None)).await.unwrap_err();
        assert!(error.to_string().contains("use run_cli"));
    }

    #[tokio::test]
    async fn validates_input() {
        let packet_tracer = ScriptedPacketTracer::new(|_| panic!("must not call Packet Tracer"));
        for bad in [Some(0), Some(301)] {
            assert!(matches!(
                run(&packet_tracer, &request(bad)).await,
                Err(PtError::InvalidInput(_))
            ));
        }
        let blank = HostCommandRequest {
            device: " ".into(),
            ..request(None)
        };
        assert!(matches!(
            run(&packet_tracer, &blank).await,
            Err(PtError::InvalidInput(_))
        ));
    }
}
