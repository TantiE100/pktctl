use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CliMode, enter};
use crate::{
    features::devices::describe,
    packet_tracer::{CommandStatus, PacketTracer, PtError},
};

const MODE_SWITCHES: &[&str] = &[
    "enable",
    "en",
    "configure terminal",
    "config terminal",
    "conf terminal",
    "configure t",
    "config t",
    "conf t",
];
const LEAVE: &[&str] = &["end"];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ConfigureIosRequest {
    /// Router or switch name.
    pub device: String,
    /// Configuration commands in order, for example `hostname R1`, `interface GigabitEthernet0/0`,
    /// `ip address 10.0.0.1 255.255.255.0`, `no shutdown`. A leading `enable` or
    /// `configure terminal` and a trailing `end` are handled for you.
    pub commands: Vec<String>,
    /// Run `write memory` afterwards so the configuration survives a reload.
    #[serde(default)]
    pub save: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CommandOutcome {
    pub command: String,
    pub status: CommandStatus,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ConfigureIosResult {
    pub device: String,
    pub completed: bool,
    pub applied: usize,
    pub total: usize,
    pub saved: bool,
    pub results: Vec<CommandOutcome>,
}

pub async fn configure<P: PacketTracer>(
    packet_tracer: &P,
    request: &ConfigureIosRequest,
) -> Result<ConfigureIosResult, PtError> {
    let device = request.device.trim();
    if device.is_empty() {
        return Err(PtError::InvalidInput("device is required".into()));
    }
    let commands = normalize(&request.commands);
    if commands.is_empty() {
        return Err(PtError::InvalidInput(
            "give at least one configuration command".into(),
        ));
    }
    describe(packet_tracer, device).await?;

    let mut results = Vec::with_capacity(commands.len());
    for (index, command) in commands.iter().enumerate() {
        let mode = if index == 0 {
            CliMode::Global
        } else {
            CliMode::Current
        };
        let reply = enter(packet_tracer, device, command, mode).await?;
        let accepted = reply.status == CommandStatus::Ok;
        results.push(CommandOutcome {
            command: command.clone(),
            status: reply.status,
            output: match tidy(&reply.output) {
                output if output.is_empty() => reply.status.explanation().to_owned(),
                output => output,
            },
        });
        if !accepted {
            break;
        }
    }

    enter(packet_tracer, device, "end", CliMode::Current).await?;
    let applied = results
        .iter()
        .filter(|outcome| outcome.status == CommandStatus::Ok)
        .count();
    let completed = applied == commands.len();

    let saved = if request.save && completed {
        let reply = enter(packet_tracer, device, "write memory", CliMode::Enable).await?;
        reply.status == CommandStatus::Ok && reply.output.contains("[OK]")
    } else {
        false
    };

    Ok(ConfigureIosResult {
        device: device.to_owned(),
        completed,
        applied,
        total: commands.len(),
        saved,
        results,
    })
}

fn normalize(commands: &[String]) -> Vec<String> {
    let mut commands: Vec<String> = commands
        .iter()
        .map(|command| command.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|command| !command.is_empty())
        .collect();
    let leading = commands
        .iter()
        .take_while(|command| MODE_SWITCHES.contains(&command.to_ascii_lowercase().as_str()))
        .count();
    commands.drain(..leading);
    while commands
        .last()
        .is_some_and(|command| LEAVE.contains(&command.to_ascii_lowercase().as_str()))
    {
        commands.pop();
    }
    commands
}

fn tidy(output: &str) -> String {
    output.trim().to_owned()
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

    async fn router() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        let request = AddDeviceRequest {
            model: "2911".into(),
            name: Some("R1".into()),
            ..AddDeviceRequest::default()
        };
        add(&packet_tracer, &request).await.unwrap();
        (canvas, packet_tracer)
    }

    fn request(commands: &[&str], save: bool) -> ConfigureIosRequest {
        ConfigureIosRequest {
            device: "R1".into(),
            commands: commands.iter().map(ToString::to_string).collect(),
            save,
        }
    }

    #[test]
    fn strips_mode_switches_and_blank_lines() {
        let commands = normalize(
            &[
                "enable",
                "  conf   t ",
                "hostname R1",
                "",
                "interface  GigabitEthernet0/0",
                "end",
            ]
            .map(String::from),
        );
        assert_eq!(commands, ["hostname R1", "interface GigabitEthernet0/0"]);
    }

    #[tokio::test]
    async fn enters_global_then_follows_the_prompt_and_leaves() {
        let (canvas, packet_tracer) = router().await;
        let result = configure(
            &packet_tracer,
            &request(
                &[
                    "configure terminal",
                    "hostname EDGE",
                    "interface GigabitEthernet0/0",
                    "ip address 10.0.0.1 255.255.255.0",
                    "no shutdown",
                ],
                true,
            ),
        )
        .await
        .unwrap();
        assert!(result.completed && result.saved);
        assert_eq!((result.applied, result.total), (4, 4));
        assert_eq!(
            canvas.cli_history("R1"),
            [
                ("global", "hostname EDGE"),
                ("", "interface GigabitEthernet0/0"),
                ("", "ip address 10.0.0.1 255.255.255.0"),
                ("", "no shutdown"),
                ("", "end"),
                ("enable", "write memory"),
            ]
            .map(|(mode, command)| (mode.to_owned(), command.to_owned()))
        );
    }

    #[tokio::test]
    async fn stops_at_the_first_rejected_command_and_does_not_save() {
        let (canvas, packet_tracer) = router().await;
        let result = configure(
            &packet_tracer,
            &request(&["hostname EDGE", "bogus command", "no shutdown"], true),
        )
        .await
        .unwrap();
        assert!(!result.completed && !result.saved);
        assert_eq!(result.applied, 1);
        assert_eq!(result.results[1].status, CommandStatus::Invalid);
        assert!(result.results[1].output.contains("Invalid input"));
        let typed: Vec<_> = canvas
            .cli_history("R1")
            .into_iter()
            .map(|(_, command)| command)
            .collect();
        assert_eq!(typed, ["hostname EDGE", "bogus command", "end"]);
    }

    #[tokio::test]
    async fn refuses_empty_blocks_and_missing_devices() {
        let (_canvas, packet_tracer) = router().await;
        assert!(matches!(
            configure(&packet_tracer, &request(&["conf t", "end"], false)).await,
            Err(PtError::InvalidInput(_))
        ));
        let ghost = ConfigureIosRequest {
            device: "Ghost".into(),
            ..request(&["hostname X"], false)
        };
        assert!(matches!(
            configure(&packet_tracer, &ghost).await,
            Err(PtError::NotFound(_))
        ));
    }
}
