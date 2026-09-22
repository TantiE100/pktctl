mod text;

use ptmp::{Call, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::paths::app_window,
    packet_tracer::{
        PacketTracer, PtError, expect_bool, expect_integer, expect_number, expect_text,
    },
    server::PktctlServer,
};

pub use text::html_to_text;

const MILLISECONDS: i64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Tally {
    pub correct: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ActivityStatus {
    pub file: String,
    pub is_activity: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_percent: Option<f64>,
    /// Assessment items (the Check Results tree).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Tally>,
    /// Assessment points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Tally>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction_pages: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_elapsed: Option<i64>,
    /// Seconds left when the activity has a countdown timer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_confirmed: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct InstructionsRequest {
    /// Page number starting at 1. Defaults to 1.
    #[serde(default)]
    pub page: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Instructions {
    pub page: i64,
    pub pages: i64,
    /// The page as readable text.
    pub text: String,
    /// The page as Packet Tracer stores it.
    pub html: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ActivityCheck {
    pub status: ActivityStatus,
    pub connectivity: Tally,
    /// One line per connectivity test, as Packet Tracer reports it.
    pub connectivity_results: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DescriptionRequest {
    /// New description (HTML or plain text). Omit to only read it.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Description {
    pub text: String,
    pub html: String,
}

fn active_file() -> Call {
    app_window().method("getActiveFile", [])
}

async fn get<P: PacketTracer>(packet_tracer: &P, method: &str) -> Result<Value, PtError> {
    packet_tracer.call(active_file().method(method, [])).await
}

async fn require_activity<P: PacketTracer>(packet_tracer: &P) -> Result<(), PtError> {
    let is_activity = get(packet_tracer, "isActivityFile").await?;
    if expect_bool(&is_activity, "isActivityFile")? {
        Ok(())
    } else {
        Err(PtError::InvalidInput(
            "the open network is not an activity (.pka); open one with open_network".into(),
        ))
    }
}

fn count(value: &Value, what: &str) -> Result<i64, PtError> {
    #[allow(clippy::cast_possible_truncation)]
    Ok(expect_number(value, what)?.round() as i64)
}

pub async fn status<P: PacketTracer>(packet_tracer: &P) -> Result<ActivityStatus, PtError> {
    let (file, is_activity) = tokio::try_join!(
        get(packet_tracer, "getSavedFilename"),
        get(packet_tracer, "isActivityFile"),
    )?;
    let file = expect_text(&file, "file name")?;
    if !expect_bool(&is_activity, "isActivityFile")? {
        return Ok(ActivityStatus {
            file,
            is_activity: false,
            percent_complete: None,
            score_percent: None,
            items: None,
            points: None,
            instruction_pages: None,
            seconds_elapsed: None,
            seconds_left: None,
            password_confirmed: None,
        });
    }
    let (percent, score, items, correct_items, points, correct_points) = tokio::try_join!(
        get(packet_tracer, "getPercentageComplete"),
        get(packet_tracer, "getPercentageCompleteScore"),
        get(packet_tracer, "getAssessmentItemsCount"),
        get(packet_tracer, "getCorrectAssessmentItemsCount"),
        get(packet_tracer, "getAssessmentScoreCount"),
        get(packet_tracer, "getCorrectAssessmentScoreCount"),
    )?;
    let (pages, elapsed, countdown, left, confirmed) = tokio::try_join!(
        get(packet_tracer, "getInstructionCount"),
        get(packet_tracer, "getTimeElapsed"),
        get(packet_tracer, "getCountDownTime"),
        get(packet_tracer, "getCountDownTimeLeft"),
        get(packet_tracer, "isPasswordConfirmed"),
    )?;
    let timer = packet_tracer
        .call(active_file().method("getTimerType", []))
        .await?;
    let counts_down = expect_integer(&timer, "timer type")? != 0;
    Ok(ActivityStatus {
        file,
        is_activity: true,
        percent_complete: Some(expect_number(&percent, "percentage complete")?),
        score_percent: Some(expect_number(&score, "score")?),
        items: Some(Tally {
            correct: count(&correct_items, "correct items")?,
            total: count(&items, "items")?,
        }),
        points: Some(Tally {
            correct: count(&correct_points, "correct points")?,
            total: count(&points, "points")?,
        }),
        instruction_pages: Some(expect_integer(&pages, "instruction pages")?),
        seconds_elapsed: Some(expect_integer(&elapsed, "time elapsed")? / MILLISECONDS),
        seconds_left: counts_down
            .then(|| expect_integer(&left, "time left").map(|left| left / MILLISECONDS))
            .transpose()?
            .filter(|_| expect_integer(&countdown, "countdown").is_ok_and(|total| total > 0)),
        password_confirmed: Some(expect_bool(&confirmed, "password confirmed")?),
    })
}

pub async fn instructions<P: PacketTracer>(
    packet_tracer: &P,
    request: &InstructionsRequest,
) -> Result<Instructions, PtError> {
    require_activity(packet_tracer).await?;
    let pages = get(packet_tracer, "getInstructionCount").await?;
    let pages = expect_integer(&pages, "instruction pages")?;
    let page = request.page.unwrap_or(1);
    if page < 1 || page > pages {
        return Err(PtError::InvalidInput(format!(
            "page must be between 1 and {pages}"
        )));
    }
    let html = packet_tracer
        .call(active_file().method(
            "getInstruction",
            [Value::Int(i32::try_from(page - 1).unwrap_or_default())],
        ))
        .await?;
    let html = expect_text(&html, "instructions")?;
    Ok(Instructions {
        page,
        pages,
        text: html_to_text(&html),
        html,
    })
}

pub async fn check<P: PacketTracer>(packet_tracer: &P) -> Result<ActivityCheck, PtError> {
    require_activity(packet_tracer).await?;
    packet_tracer
        .call(active_file().method("runConnectivityTests", []))
        .await?;
    let (total, correct) = tokio::try_join!(
        get(packet_tracer, "getConnectivityCount"),
        get(packet_tracer, "getLastConnectivityTestCorrectCount"),
    )?;
    let total = count(&total, "connectivity tests")?;
    let mut connectivity_results = Vec::new();
    for index in 0..total {
        let line = packet_tracer
            .call(active_file().method(
                "getLastConnectivityTestResultAt",
                [Value::Int(i32::try_from(index).unwrap_or_default())],
            ))
            .await?;
        connectivity_results.push(expect_text(&line, "connectivity result")?);
    }
    Ok(ActivityCheck {
        status: status(packet_tracer).await?,
        connectivity: Tally {
            correct: expect_integer(&correct, "correct connectivity tests")?,
            total,
        },
        connectivity_results,
    })
}

pub async fn reset<P: PacketTracer>(packet_tracer: &P) -> Result<ActivityStatus, PtError> {
    require_activity(packet_tracer).await?;
    packet_tracer
        .call(active_file().method("resetActivity", []))
        .await?;
    status(packet_tracer).await
}

pub async fn description<P: PacketTracer>(
    packet_tracer: &P,
    request: &DescriptionRequest,
) -> Result<Description, PtError> {
    if let Some(text) = &request.text {
        packet_tracer
            .call(active_file().method("setNetworkDescription", [Value::qstring(text)]))
            .await?;
    }
    let html = get(packet_tracer, "getNetworkDescription").await?;
    let html = expect_text(&html, "description")?;
    Ok(Description {
        text: html_to_text(&html),
        html,
    })
}

#[tool_router(router = activity_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "activity_status",
        description = "Progress of the open activity (.pka): percentage complete, score, \
                       assessment items and points, instruction pages, time elapsed or left. \
                       For a plain network it only says it is not an activity.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn activity_status_tool(&self) -> Result<Json<ActivityStatus>, String> {
        status(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "activity_instructions",
        description = "Read a page of the open activity's instructions, as text and as HTML.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn activity_instructions_tool(
        &self,
        Parameters(request): Parameters<InstructionsRequest>,
    ) -> Result<Json<Instructions>, String> {
        instructions(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "check_activity",
        description = "Run the open activity's connectivity tests and return them with the \
                       current completion and score, like Check Results.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn check_activity_tool(&self) -> Result<Json<ActivityCheck>, String> {
        check(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "reset_activity",
        description = "Reset the open activity to its initial network, discarding the work \
                       done in it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn reset_activity_tool(&self) -> Result<Json<ActivityStatus>, String> {
        reset(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "network_description",
        description = "Read the open file's description (the Description button in Packet \
                       Tracer), or replace it with `text`.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn network_description_tool(
        &self,
        Parameters(request): Parameters<DescriptionRequest>,
    ) -> Result<Json<Description>, String> {
        description(self.packet_tracer(), &request)
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
        packet_tracer::scripted::ScriptedPacketTracer,
        testing::{ActivityFixture, Canvas},
    };

    fn lab_activity() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        canvas.open_activity(
            "/labs/vlans.pka",
            ActivityFixture {
                instructions: vec![
                    "<h1>VLANs</h1><p>Create VLAN&nbsp;10</p>".into(),
                    "<p>Ping PC2</p>".into(),
                ],
                items: (3, 4),
                connectivity: vec![
                    ("PC1 to PC2: success".into(), true),
                    ("PC1 to Server: failed".into(), false),
                ],
                seconds_left: Some(600),
            },
        );
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        (canvas, packet_tracer)
    }

    #[tokio::test]
    async fn reports_progress_of_an_activity() {
        let (_canvas, packet_tracer) = lab_activity();
        let status = status(&packet_tracer).await.unwrap();
        assert!(status.is_activity);
        assert_eq!(status.percent_complete, Some(75.0));
        assert_eq!(
            status.items,
            Some(Tally {
                correct: 3,
                total: 4
            })
        );
        assert_eq!(status.instruction_pages, Some(2));
        assert_eq!(status.seconds_elapsed, Some(125));
        assert_eq!(status.seconds_left, Some(600));
    }

    #[tokio::test]
    async fn reads_instruction_pages_as_text() {
        let (_canvas, packet_tracer) = lab_activity();
        let second = instructions(&packet_tracer, &InstructionsRequest { page: Some(2) })
            .await
            .unwrap();
        assert_eq!(
            (second.page, second.pages, second.text.as_str()),
            (2, 2, "Ping PC2")
        );
        let first = instructions(&packet_tracer, &InstructionsRequest::default())
            .await
            .unwrap();
        assert_eq!(first.text, "VLANs\nCreate VLAN 10");
        assert!(
            instructions(&packet_tracer, &InstructionsRequest { page: Some(3) })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn runs_connectivity_tests_and_resets() {
        let (_canvas, packet_tracer) = lab_activity();
        let checked = check(&packet_tracer).await.unwrap();
        assert_eq!(
            checked.connectivity,
            Tally {
                correct: 1,
                total: 2
            }
        );
        assert_eq!(checked.connectivity_results[1], "PC1 to Server: failed");
        let reset_status = reset(&packet_tracer).await.unwrap();
        assert_eq!(reset_status.seconds_elapsed, Some(0));
    }

    #[tokio::test]
    async fn plain_networks_are_not_activities_but_have_descriptions() {
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::new(Canvas::new()));
        assert!(!status(&packet_tracer).await.unwrap().is_activity);
        assert!(
            check(&packet_tracer)
                .await
                .unwrap_err()
                .to_string()
                .contains("not an activity")
        );
        let described = description(
            &packet_tracer,
            &DescriptionRequest {
                text: Some("<p>Red GAMC &amp; VLANs</p>".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(described.text, "Red GAMC & VLANs");
    }
}
