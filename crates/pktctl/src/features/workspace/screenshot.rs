use std::{path::Path, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use ptmp::Value;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    desktop::Desktop,
    features::paths::{app_window, logical_workspace},
    packet_tracer::{PacketTracer, PtError, expect_bool},
};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const REDRAW_PAUSE: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// The logical workspace, rendered by Packet Tracer.
    #[default]
    Logical,
    /// The physical workspace from Intercity, captured from Packet Tracer's window.
    Physical,
    /// The physical workspace inside the main wiring closet, showing its rack.
    PhysicalRack,
    /// Packet Tracer's window as it is now, dialogs included.
    Window,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ScreenshotRequest {
    /// `logical` (default), `physical` (Intercity), `physical_rack` (main wiring closet), or
    /// `window`. All but `logical` capture Packet Tracer's own window through the operating
    /// system.
    #[serde(default)]
    pub view: View,
    /// Also write the PNG to this absolute path, for example for a lab report.
    #[serde(default)]
    pub save_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub png: Vec<u8>,
    pub saved_to: Option<String>,
}

impl Screenshot {
    pub fn base64(&self) -> String {
        STANDARD.encode(&self.png)
    }
}

pub async fn capture<P: PacketTracer>(
    packet_tracer: &P,
    desktop: &dyn Desktop,
    request: &ScreenshotRequest,
) -> Result<Screenshot, PtError> {
    let target = request
        .save_to
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(png_path)
        .transpose()?;

    let png = match request.view {
        View::Logical => {
            let image = packet_tracer
                .call(logical_workspace().method("getWorkspaceImage", [Value::qstring("PNG")]))
                .await?;
            image.into_bytes().ok_or_else(|| {
                PtError::UnexpectedReply("the workspace image should be a byte list".into())
            })?
        }
        View::Window => desktop.capture_packet_tracer()?,
        View::Physical => physical(packet_tracer, desktop, "switchToTopView").await?,
        View::PhysicalRack => physical(packet_tracer, desktop, "switchToHomeRack").await?,
    };
    if !png.starts_with(PNG_SIGNATURE) {
        return Err(PtError::UnexpectedReply(
            "the workspace image is not a PNG".into(),
        ));
    }

    if let Some(path) = &target {
        std::fs::write(path, &png)
            .map_err(|error| PtError::InvalidInput(format!("could not write `{path}`: {error}")))?;
    }
    Ok(Screenshot {
        png,
        saved_to: target,
    })
}

async fn physical<P: PacketTracer>(
    packet_tracer: &P,
    desktop: &dyn Desktop,
    place: &str,
) -> Result<Vec<u8>, PtError> {
    let switch = app_window().method("getPLSwitch", []);
    let was_physical = packet_tracer
        .call(app_window().method("isPhysicalMode", []))
        .await?;
    let was_physical = expect_bool(&was_physical, "isPhysicalMode")?;
    if !was_physical {
        packet_tracer
            .call(switch.clone().method("showPhysicalMode", []))
            .await?;
    }
    packet_tracer
        .call(
            app_window()
                .method("getPhysicalToolbar", [])
                .method(place, []),
        )
        .await?;
    tokio::time::sleep(REDRAW_PAUSE).await;
    let captured = desktop.capture_packet_tracer();
    if !was_physical {
        packet_tracer
            .call(switch.method("showLogicalMode", []))
            .await?;
    }
    captured
}

fn png_path(path: &str) -> Result<String, PtError> {
    let candidate = Path::new(path);
    let is_png = candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if candidate.is_absolute() && is_png {
        Ok(path.to_owned())
    } else {
        Err(PtError::InvalidInput(format!(
            "`{path}` must be an absolute path ending in .png"
        )))
    }
}
