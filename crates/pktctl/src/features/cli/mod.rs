mod configure;

use ptmp::Value;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use configure::{CommandOutcome, ConfigureIosRequest, ConfigureIosResult, configure};

use crate::{
    features::paths::device,
    packet_tracer::{CommandStatus, PacketTracer, PtError, expect_integer, expect_text},
    server::PktctlServer,
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CliRequest {
    /// Device name exactly as shown by `list_devices`.
    pub device: String,
    /// One IOS command, for example `show ip interface brief`.
    pub command: String,
    /// Mode to run the command in. Defaults to `enable`.
    #[serde(default)]
    pub mode: CliMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CliMode {
    User,
    #[default]
    Enable,
    Global,
    Current,
}

impl CliMode {
    pub(crate) fn as_packet_tracer(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Enable => "enable",
            Self::Global => "global",
            Self::Current => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CliResult {
    pub status: CommandStatus,
    pub output: String,
}

pub async fn run<P: PacketTracer>(
    packet_tracer: &P,
    request: &CliRequest,
) -> Result<CliResult, PtError> {
    let device = request.device.trim();
    let command = request.command.trim();
    if device.is_empty() || command.is_empty() {
        return Err(PtError::InvalidInput(
            "device and command are required".into(),
        ));
    }
    enter(packet_tracer, device, command, request.mode).await
}

pub(crate) async fn enter<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
    command: &str,
    mode: CliMode,
) -> Result<CliResult, PtError> {
    let call = device(device_name).method(
        "enterCommand",
        [
            Value::string(command),
            Value::string(mode.as_packet_tracer()),
        ],
    );
    let reply = packet_tracer.call(call).await.map_err(|error| match error {
        PtError::NotFound(_) => PtError::NotFound(format!("device `{device_name}`")),
        PtError::Rejected(reason) if reason.contains("enterCommand") => PtError::Rejected(format!(
            "{reason}; `{device_name}` has no IOS console, use run_host_command for PCs and servers"
        )),
        other => other,
    })?;
    let Some((status, output)) = reply.clone().into_pair() else {
        return Err(PtError::UnexpectedReply(format!(
            "enterCommand should return a status and output, got {reply:?}"
        )));
    };

    let code = expect_integer(&status, "command status")?;
    let status = CommandStatus::from_code(code)
        .ok_or_else(|| PtError::UnexpectedReply(format!("unknown command status {code}")))?;
    Ok(CliResult {
        status,
        output: expect_text(&output, "command output")?,
    })
}

#[tool_router(router = cli_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "configure_ios",
        description = "Apply a block of IOS configuration commands to a router or switch, as if \
                       typed after `configure terminal`: the first command enters global \
                       configuration and the rest follow the prompt, so `interface ...` sub-modes \
                       work. Stops at the first command IOS rejects, always leaves \
                       configuration mode, and runs `write memory` when `save` is true.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn configure_ios_tool(
        &self,
        Parameters(request): Parameters<ConfigureIosRequest>,
    ) -> Result<Json<ConfigureIosResult>, String> {
        configure(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "run_cli",
        description = "Run one IOS command on a router or switch and return its console output. \
                       `status` tells whether IOS accepted the command.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn run_cli_tool(
        &self,
        Parameters(request): Parameters<CliRequest>,
    ) -> Result<Json<CliResult>, String> {
        run(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use ptmp::Call;

    use super::*;
    use crate::packet_tracer::scripted::ScriptedPacketTracer;

    fn request(command: &str, mode: CliMode) -> CliRequest {
        CliRequest {
            device: "R1".into(),
            command: command.into(),
            mode,
        }
    }

    fn console(status: i32, output: &str) -> Value {
        Value::Pair(
            Box::new(Value::Int(status)),
            Box::new(Value::string(output)),
        )
    }

    #[tokio::test]
    async fn sends_command_and_mode_exactly_as_packet_tracer_expects() {
        let packet_tracer = ScriptedPacketTracer::new(|call| {
            let expected = Call::root("network")
                .method("getDevice", [Value::qstring("R1")])
                .method(
                    "enterCommand",
                    [Value::string("hostname CORE"), Value::string("global")],
                );
            assert_eq!(call, &expected);
            Ok(console(0, ""))
        });
        let result = run(&packet_tracer, &request("hostname CORE", CliMode::Global))
            .await
            .unwrap();
        assert_eq!(result.status, CommandStatus::Ok);
    }

    #[tokio::test]
    async fn reports_invalid_commands_without_failing() {
        let packet_tracer =
            ScriptedPacketTracer::new(|_| Ok(console(2, "% Invalid input detected")));
        let result = run(&packet_tracer, &request("shw run", CliMode::Enable))
            .await
            .unwrap();
        assert_eq!(result.status, CommandStatus::Invalid);
        assert!(result.output.contains("Invalid input"));
    }

    #[tokio::test]
    async fn current_mode_is_sent_as_blank() {
        let packet_tracer = ScriptedPacketTracer::new(|call| {
            assert_eq!(call.steps()[2].args[1], Value::string(""));
            Ok(console(0, ""))
        });
        run(&packet_tracer, &request("exit", CliMode::Current))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_blank_input_before_calling_packet_tracer() {
        let packet_tracer = ScriptedPacketTracer::new(|_| panic!("must not call Packet Tracer"));
        let error = run(&packet_tracer, &request("  ", CliMode::Enable))
            .await
            .unwrap_err();
        assert!(matches!(error, PtError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn surfaces_non_ios_devices_as_rejections() {
        let packet_tracer = ScriptedPacketTracer::new(|_| {
            Err(PtError::Rejected(
                r#"Pc: IPC call "enterCommand" not found"#.into(),
            ))
        });
        let error = run(&packet_tracer, &request("show version", CliMode::Enable))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("enterCommand"));
    }
}
