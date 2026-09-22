mod configure;

use std::time::Duration;

use ptmp::Value;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use configure::{CommandOutcome, ConfigureIosRequest, ConfigureIosResult, configure};

use crate::{
    features::{
        devices::{describe, ready_console},
        paths::device,
        terminal::{self, Interrupt, Terminal, TerminalRun},
    },
    packet_tracer::{
        CommandStatus, PacketTracer, PtError, expect_integer, expect_text, kinds::runs_ios,
    },
    server::PktctlServer,
};

const MODE_CHANGE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CliRequest {
    /// Device name exactly as shown by `list_devices`.
    pub device: String,
    /// One IOS command, for example `show ip interface brief`. To answer a question the
    /// previous command left open (`question` in its reply), send the answer here with mode
    /// `current`; an empty command presses Enter, which confirms `[confirm]`.
    pub command: String,
    /// Mode to run the command in. Defaults to `enable`.
    #[serde(default)]
    pub mode: CliMode,
    /// Seconds to wait for the command to finish. Defaults to 30, maximum 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
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
) -> Result<TerminalRun, PtError> {
    let device_name = request.device.trim();
    let command = request.command.trim();
    if device_name.is_empty() {
        return Err(PtError::InvalidInput("device is required".into()));
    }
    if command.is_empty() && request.mode != CliMode::Current {
        return Err(PtError::InvalidInput(
            "command is required; an empty command (Enter) only answers a question, in mode \
             `current`"
                .into(),
        ));
    }
    let timeout = terminal::timeout(request.timeout_secs)?;
    let target = describe(packet_tracer, device_name).await?;
    if !runs_ios(&target.kind) {
        return Err(PtError::InvalidInput(format!(
            "`{device_name}` is a {} and has no IOS console; use run_host_command for PCs and \
             servers",
            target.kind
        )));
    }

    ready_console(packet_tracer, device_name).await?;
    if request.mode != CliMode::Current {
        let prompt = packet_tracer
            .call(
                device(device_name)
                    .method("getCommandLine", [])
                    .method("getPrompt", []),
            )
            .await?;
        if let Some(question) = terminal::pending_question(prompt.as_str().unwrap_or_default()) {
            return Err(PtError::InvalidInput(format!(
                "`{device_name}` is waiting on `{question}`; answer it first with mode `current` \
                 (an empty command presses Enter)"
            )));
        }
    }
    let console = Terminal::open(
        packet_tracer,
        device(device_name).method("getCommandLine", []),
        Interrupt::CtrlShift6,
    )
    .await?;
    switch_mode(&console, request.mode).await?;
    console.run(command, timeout).await
}

async fn switch_mode<P: PacketTracer>(
    console: &Terminal<'_, P>,
    target: CliMode,
) -> Result<(), PtError> {
    if target == CliMode::Current {
        return Ok(());
    }
    let current = console.mode().await?;
    for step in mode_path(&current, target) {
        let run = console.run(step, MODE_CHANGE_TIMEOUT).await?;
        if !run.finished || run.status != Some(CommandStatus::Ok) {
            return Err(PtError::Rejected(format!(
                "could not reach {} mode: `{step}` did not complete (is an enable password \
                 set?). Console said: {}",
                target.as_packet_tracer(),
                run.output.trim()
            )));
        }
    }
    let reached = console.mode().await?;
    if reached == target.as_packet_tracer() {
        Ok(())
    } else {
        Err(PtError::Rejected(format!(
            "the console is in {reached} mode instead of {}",
            target.as_packet_tracer()
        )))
    }
}

fn mode_path(current: &str, target: CliMode) -> &'static [&'static str] {
    match (target, current) {
        (CliMode::Current, _)
        | (CliMode::User, "user")
        | (CliMode::Enable, "enable")
        | (CliMode::Global, "global") => &[],
        (CliMode::User, "enable") => &["disable"],
        (CliMode::User, _) => &["end", "disable"],
        (CliMode::Enable, "user") => &["enable"],
        (CliMode::Enable, _) => &["end"],
        (CliMode::Global, "user") => &["enable", "configure terminal"],
        (CliMode::Global, "enable") => &["configure terminal"],
        (CliMode::Global, _) => &["end", "configure terminal"],
    }
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
        description = "Type one IOS command at the console of a router or switch and return \
                       its output, including commands that take time such as ping or \
                       traceroute. `status` tells whether IOS accepted the command. A command \
                       still running after `timeout_secs` is stopped with Ctrl+Shift+6 and \
                       comes back with `finished: false` and the output so far. When IOS asks \
                       something ([confirm], [yes/no], Password:) the reply carries `question` \
                       and the console keeps waiting: answer with another run_cli in mode \
                       `current`, an empty command for Enter.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn run_cli_tool(
        &self,
        Parameters(request): Parameters<CliRequest>,
    ) -> Result<Json<TerminalRun>, String> {
        run(self.packet_tracer(), &request)
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

    async fn lab() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        for (model, name) in [("2911", "R1"), ("PC-PT", "PC1")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        (canvas, packet_tracer)
    }

    fn request(command: &str, mode: CliMode) -> CliRequest {
        CliRequest {
            device: "R1".into(),
            command: command.into(),
            mode,
            timeout_secs: None,
        }
    }

    #[tokio::test]
    async fn leaves_questions_open_for_an_answer() {
        let (canvas, packet_tracer) = lab().await;
        let asked = run(&packet_tracer, &request("reload", CliMode::Enable))
            .await
            .unwrap();
        assert!(!asked.finished);
        assert_eq!(
            asked.question.as_deref(),
            Some("Proceed with reload? [confirm]")
        );

        let blocked = run(&packet_tracer, &request("show clock", CliMode::Enable))
            .await
            .unwrap_err();
        assert!(blocked.to_string().contains("waiting on"), "{blocked}");
        let empty = run(&packet_tracer, &request("", CliMode::Enable))
            .await
            .unwrap_err();
        assert!(matches!(empty, PtError::InvalidInput(_)), "{empty}");

        let answered = run(&packet_tracer, &request("", CliMode::Current))
            .await
            .unwrap();
        assert!(answered.finished && answered.question.is_none());
        assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router>"));
    }

    #[tokio::test]
    async fn waits_for_commands_that_print_over_time() {
        let (_canvas, packet_tracer) = lab().await;
        let result = run(&packet_tracer, &request("ping 10.0.0.2", CliMode::Enable))
            .await
            .unwrap();
        assert!(result.finished);
        assert_eq!(result.status, Some(CommandStatus::Ok));
        assert!(result.output.starts_with("Sending 5"), "{}", result.output);
        assert!(result.output.contains("!!!!!\nSuccess rate is 100 percent"));
        assert!(!result.output.ends_with("Router#"));
    }

    #[tokio::test]
    async fn walks_the_console_to_the_requested_mode() {
        let (canvas, packet_tracer) = lab().await;
        run(&packet_tracer, &request("hostname CORE", CliMode::Global))
            .await
            .unwrap();
        run(&packet_tracer, &request("show clock", CliMode::User))
            .await
            .unwrap();
        run(&packet_tracer, &request("interface g0/0", CliMode::Global))
            .await
            .unwrap();
        run(&packet_tracer, &request("show ip route", CliMode::Enable))
            .await
            .unwrap();
        let typed: Vec<(String, String)> = canvas.cli_history("R1").into_iter().collect();
        let typed: Vec<(&str, &str)> = typed
            .iter()
            .map(|(mode, command)| (mode.as_str(), command.as_str()))
            .collect();
        assert_eq!(
            typed,
            [
                ("user", "enable"),
                ("enable", "configure terminal"),
                ("global", "hostname CORE"),
                ("global", "end"),
                ("enable", "disable"),
                ("user", "show clock"),
                ("user", "enable"),
                ("enable", "configure terminal"),
                ("global", "interface g0/0"),
                ("intG", "end"),
                ("enable", "show ip route"),
            ]
        );
    }

    #[tokio::test]
    async fn current_mode_types_where_the_console_is() {
        let (canvas, packet_tracer) = lab().await;
        run(&packet_tracer, &request("show version", CliMode::Current))
            .await
            .unwrap();
        assert_eq!(
            canvas.cli_history("R1"),
            [("user".to_owned(), "show version".to_owned())]
        );
    }

    #[tokio::test]
    async fn pages_through_long_output() {
        let (_canvas, packet_tracer) = lab().await;
        let result = run(
            &packet_tracer,
            &request("show running-config", CliMode::Enable),
        )
        .await
        .unwrap();
        assert!(result.finished);
        assert_eq!(result.output, "hostname Router\n!\nend\n");
    }

    #[tokio::test]
    async fn reports_invalid_commands_without_failing() {
        let (_canvas, packet_tracer) = lab().await;
        let result = run(&packet_tracer, &request("bogus run", CliMode::Enable))
            .await
            .unwrap();
        assert_eq!(result.status, Some(CommandStatus::Invalid));
        assert!(result.output.contains("Invalid input"));
    }

    #[tokio::test(start_paused = true)]
    async fn returns_what_it_has_when_the_command_outlives_the_timeout() {
        let (_canvas, packet_tracer) = lab().await;
        let result = run(
            &packet_tracer,
            &CliRequest {
                timeout_secs: Some(3),
                ..request("debug hang", CliMode::Enable)
            },
        )
        .await
        .unwrap();
        assert!(!result.finished);
        assert_eq!(result.status, None);
        assert_eq!(result.output, "waiting forever\n\n");

        let next = run(&packet_tracer, &request("show clock", CliMode::Enable))
            .await
            .unwrap();
        assert!(next.finished, "the interrupt must free the console");
    }

    #[tokio::test]
    async fn answers_the_initial_dialog_first() {
        let (canvas, packet_tracer) = lab().await;
        canvas.reset_console("R1");
        run(&packet_tracer, &request("show version", CliMode::Enable))
            .await
            .unwrap();
        assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router#"));
    }

    #[tokio::test]
    async fn rejects_end_devices_and_bad_input() {
        let (_canvas, packet_tracer) = lab().await;
        let pc = CliRequest {
            device: "PC1".into(),
            ..request("show version", CliMode::Enable)
        };
        let error = run(&packet_tracer, &pc).await.unwrap_err();
        assert!(error.to_string().contains("run_host_command"), "{error}");

        for bad in [
            request("  ", CliMode::Enable),
            CliRequest {
                timeout_secs: Some(0),
                ..request("show version", CliMode::Enable)
            },
        ] {
            assert!(matches!(
                run(&packet_tracer, &bad).await,
                Err(PtError::InvalidInput(_))
            ));
        }
        let missing = CliRequest {
            device: "R9".into(),
            ..request("show version", CliMode::Enable)
        };
        assert!(matches!(
            run(&packet_tracer, &missing).await,
            Err(PtError::NotFound(_))
        ));
    }

    #[test]
    fn mode_paths_follow_ios() {
        assert_eq!(mode_path("intG", CliMode::User), ["end", "disable"]);
        assert_eq!(
            mode_path("user", CliMode::Global),
            ["enable", "configure terminal"]
        );
        assert!(mode_path("global", CliMode::Global).is_empty());
        assert!(mode_path("intG", CliMode::Current).is_empty());
    }
}
