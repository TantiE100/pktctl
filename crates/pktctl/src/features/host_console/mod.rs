use ptmp::{Call, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    features::terminal::{self, Interrupt, Terminal, TerminalRun},
    packet_tracer::{PacketTracer, PtError},
    server::PktctlServer,
};

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

pub async fn run<P: PacketTracer>(
    packet_tracer: &P,
    request: &HostCommandRequest,
) -> Result<TerminalRun, PtError> {
    let device = request.device.trim();
    let command = request.command.trim();
    if device.is_empty() || command.is_empty() {
        return Err(PtError::InvalidInput(
            "device and command are required".into(),
        ));
    }
    let timeout = terminal::timeout(request.timeout_secs)?;

    let prompt = Call::root("network")
        .method("getDevice", [Value::qstring(device)])
        .method("getCommandPrompt", []);
    let terminal = Terminal::open(packet_tracer, prompt, Interrupt::CtrlC)
        .await
        .map_err(explain_non_hosts)?;
    terminal.run(command, timeout).await
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
                       Waits until the command finishes; a command still running after \
                       `timeout_secs` (ping -t) is stopped with Ctrl+C and comes back with \
                       `finished: false` and the output so far.",
        annotations(read_only_hint = false, open_world_hint = false)
    )]
    async fn run_host_command_tool(
        &self,
        Parameters(request): Parameters<HostCommandRequest>,
    ) -> Result<Json<TerminalRun>, String> {
        run(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use ptmp::Event;

    use super::*;
    use crate::{
        features::terminal::events,
        packet_tracer::{
            CommandStatus,
            scripted::{Emitter, ScriptedPacketTracer, methods},
        },
    };

    const TERMINAL_ID: &str = "{terminal-pc1}";

    fn written(text: &str) -> Event {
        events::written(TERMINAL_ID, text)
    }

    fn ended(status: i32) -> Event {
        events::ended(TERMINAL_ID, "ping 10.0.0.2", status)
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
                    emitter.emit(events::more(TERMINAL_ID));
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
            TerminalRun {
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
    async fn leaves_out_the_echo_of_the_typed_command() {
        let packet_tracer = pc(|emitter| {
            emitter.emit(written("ping 10.0.0.2\n"));
            emitter.emit(written("Reply from 10.0.0.2\n"));
            emitter.emit(ended(0));
        });
        assert_eq!(
            run(&packet_tracer, &request(None)).await.unwrap().output,
            "Reply from 10.0.0.2\n"
        );
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
    async fn interrupts_commands_that_outlive_the_timeout() {
        let packet_tracer =
            ScriptedPacketTracer::with_events(|call, emitter| match methods(call).as_slice() {
                [.., "getObjectUuid"] => Ok(Value::Uuid(TERMINAL_ID.into())),
                [.., "enterCommand"] => {
                    emitter.emit(written("Reply from 10.0.0.2\n"));
                    Ok(Value::Void)
                }
                [.., "enterChar"] => {
                    assert_eq!(call.steps()[3].args, [Value::Byte(3), Value::Int(0)]);
                    emitter.emit(written("^C\n"));
                    emitter.emit(ended(0));
                    Ok(Value::Void)
                }
                other => panic!("unexpected call {other:?}"),
            });
        let result = run(&packet_tracer, &request(Some(5))).await.unwrap();
        assert!(!result.finished);
        assert_eq!(result.status, None);
        assert_eq!(result.output, "Reply from 10.0.0.2\n^C\n");
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
