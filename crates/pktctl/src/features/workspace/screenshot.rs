use std::path::Path;

use base64::{Engine, engine::general_purpose::STANDARD};
use ptmp::Value;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    features::paths::logical_workspace,
    packet_tracer::{PacketTracer, PtError},
};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ScreenshotRequest {
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
    request: &ScreenshotRequest,
) -> Result<Screenshot, PtError> {
    let target = request
        .save_to
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(png_path)
        .transpose()?;

    let image = packet_tracer
        .call(logical_workspace().method("getWorkspaceImage", [Value::qstring("PNG")]))
        .await?;
    let png = image.into_bytes().ok_or_else(|| {
        PtError::UnexpectedReply("the workspace image should be a byte list".into())
    })?;
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
