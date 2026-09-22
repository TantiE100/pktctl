use std::time::Duration;

use ptmp::{Call, Event, Subscription, Value};
use schemars::JsonSchema;
use serde::Serialize;
use tokio::{sync::broadcast::error::RecvError, time::Instant};

use crate::packet_tracer::{
    CommandStatus, Events, PacketTracer, PtError, expect_integer, expect_text,
};

const TERMINAL: &str = "TerminalLine";
const OUTPUT_WRITTEN: &str = "outputWritten";
const COMMAND_ENDED: &str = "commandEnded";
const MORE_DISPLAYED: &str = "moreDisplayed";
const SPACE: i8 = 32;
const NO_SPECIAL_CHAR: i32 = 0;
pub(crate) const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub(crate) const MAX_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TerminalRun {
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<CommandStatus>,
    pub output: String,
}

pub(crate) fn timeout(requested: Option<u64>) -> Result<Duration, PtError> {
    let secs = requested.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if secs == 0 || secs > MAX_TIMEOUT_SECS {
        return Err(PtError::InvalidInput(format!(
            "timeout_secs must be between 1 and {MAX_TIMEOUT_SECS}"
        )));
    }
    Ok(Duration::from_secs(secs))
}

pub(crate) struct Terminal<'a, P> {
    packet_tracer: &'a P,
    line: Call,
    id: String,
}

impl<'a, P: PacketTracer> Terminal<'a, P> {
    pub(crate) async fn open(packet_tracer: &'a P, line: Call) -> Result<Self, PtError> {
        let id = packet_tracer
            .call(line.clone().method("getObjectUuid", []))
            .await?;
        Ok(Self {
            packet_tracer,
            id: expect_text(&id, "terminal id")?,
            line,
        })
    }

    pub(crate) async fn mode(&self) -> Result<String, PtError> {
        let mode = self
            .packet_tracer
            .call(self.line.clone().method("getMode", []))
            .await?;
        expect_text(&mode, "terminal mode")
    }

    pub(crate) async fn run(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<TerminalRun, PtError> {
        let subscriptions = [OUTPUT_WRITTEN, COMMAND_ENDED, MORE_DISPLAYED]
            .map(|event| Subscription::to(TERMINAL, &self.id, event));
        let mut events = self
            .packet_tracer
            .subscribe(subscriptions[0].clone())
            .await?;
        for subscription in &subscriptions[1..] {
            self.packet_tracer.subscribe(subscription.clone()).await?;
        }

        let typed = self
            .packet_tracer
            .call(
                self.line
                    .clone()
                    .method("enterCommand", [Value::string(command)]),
            )
            .await;
        let outcome = match typed {
            Ok(_) => self.collect(&mut events, timeout).await,
            Err(error) => Err(error),
        };

        for subscription in subscriptions {
            if let Err(error) = self.packet_tracer.unsubscribe(subscription).await {
                tracing::debug!(%error, "could not unsubscribe from terminal events");
            }
        }
        outcome.map(|mut run| {
            run.output = without_echo(&run.output, command).to_owned();
            run
        })
    }

    async fn collect(
        &self,
        events: &mut Events,
        timeout: Duration,
    ) -> Result<TerminalRun, PtError> {
        let deadline = Instant::now() + timeout;
        let mut output = String::new();
        loop {
            let event = match tokio::time::timeout_at(deadline, events.recv()).await {
                Err(_) => {
                    return Ok(TerminalRun {
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
            if event.class != TERMINAL || event.object_uuid != self.id {
                continue;
            }
            match event.name.as_str() {
                OUTPUT_WRITTEN => output.push_str(first_text(&event)?),
                MORE_DISPLAYED => self.next_page().await?,
                COMMAND_ENDED => {
                    return Ok(TerminalRun {
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
            .call(self.line.clone().method(
                "enterChar",
                [Value::Byte(SPACE), Value::Int(NO_SPECIAL_CHAR)],
            ))
            .await
            .map(drop)
    }
}

fn without_echo<'o>(output: &'o str, command: &str) -> &'o str {
    output
        .strip_prefix(command)
        .map_or(output, |rest| rest.strip_prefix('\n').unwrap_or(rest))
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

#[cfg(test)]
pub(crate) mod events {
    use ptmp::{Event, Value};

    use super::{COMMAND_ENDED, MORE_DISPLAYED, OUTPUT_WRITTEN, TERMINAL};

    pub(crate) fn terminal_event(terminal: &str, name: &str, args: Vec<Value>) -> Event {
        Event {
            token: "1".into(),
            class: TERMINAL.into(),
            object_uuid: terminal.into(),
            name: name.into(),
            args,
        }
    }

    pub(crate) fn written(terminal: &str, text: &str) -> Event {
        terminal_event(
            terminal,
            OUTPUT_WRITTEN,
            vec![Value::string(text), Value::Bool(false), Value::Int(0)],
        )
    }

    pub(crate) fn ended(terminal: &str, command: &str, status: i32) -> Event {
        terminal_event(
            terminal,
            COMMAND_ENDED,
            vec![Value::string(command), Value::Int(status)],
        )
    }

    pub(crate) fn more(terminal: &str) -> Event {
        terminal_event(terminal, MORE_DISPLAYED, vec![Value::Int(0)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_typed_command_echo() {
        assert_eq!(
            without_echo("ping 1.1.1.1\n!!!!!\n", "ping 1.1.1.1"),
            "!!!!!\n"
        );
        assert_eq!(without_echo("ipconfig", "ipconfig"), "");
        assert_eq!(without_echo("\nRouter#hw\n", "shw"), "\nRouter#hw\n");
    }

    #[test]
    fn bounds_timeouts() {
        assert_eq!(timeout(None).unwrap(), Duration::from_secs(30));
        assert_eq!(timeout(Some(300)).unwrap(), Duration::from_secs(300));
        assert!(timeout(Some(0)).is_err());
        assert!(timeout(Some(301)).is_err());
    }
}
