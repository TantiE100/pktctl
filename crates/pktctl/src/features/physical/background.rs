use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::tree::{Snapshot, object};
use crate::{
    features::paths::active_workspace,
    packet_tracer::{PacketTracer, PtError},
};

/// Packet Tracer's own backgrounds, as it stores their paths.
const BUNDLED: &[(&str, &str)] = &[
    ("grid_10x10", "../art/Background/grid_10x10.png"),
    ("grid_25x25", "../art/Background/grid_25x25.png"),
    ("grid_50x50", "../art/Background/grid_50x50.png"),
    ("grid_100x100", "../art/Background/grid_100x100.png"),
    ("city", "../art/Background/gGeoViewCity.png"),
    ("building", "../art/Background/gGeoViewBuilding.png"),
    ("intercity", "../art/Background/gGeoViewInterCity.png"),
    (
        "container",
        "../art/Background/gGeoViewGenericContainer.png",
    ),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BackgroundRequest {
    /// Path of the physical location to paper, as listed by `list_locations`. Omit to change
    /// the logical workspace's background instead.
    #[serde(default)]
    pub location: Option<String>,
    /// One of Packet Tracer's own backgrounds (`grid_10x10`, `grid_25x25`, `grid_50x50`,
    /// `grid_100x100`, `city`, `building`, `intercity`, `container`) or the absolute path of
    /// an image file. An empty value clears it.
    pub image: String,
    /// Repeat the image instead of stretching it.
    #[serde(default)]
    pub tiled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Background {
    /// The location that got the background, or `logical workspace`.
    pub target: String,
    /// The path Packet Tracer now has.
    pub image: String,
    pub tiled: bool,
}

pub async fn set_background<P: PacketTracer>(
    packet_tracer: &P,
    request: &BackgroundRequest,
) -> Result<Background, PtError> {
    let image = resolve(&request.image)?;
    let target = match request.location.as_deref().map(str::trim) {
        None | Some("") => {
            packet_tracer
                .call(active_workspace().method(
                    "setLogicalBackgroundPath",
                    [Value::qstring(&image), Value::Bool(request.tiled)],
                ))
                .await?;
            "logical workspace".to_owned()
        }
        Some(path) => {
            let snapshot = Snapshot::read(packet_tracer).await?;
            let node = snapshot.by_path(path)?;
            packet_tracer
                .call(object(&node.uuid).method(
                    "setBackground",
                    [Value::qstring(&image), Value::Bool(request.tiled)],
                ))
                .await?;
            node.path.clone()
        }
    };
    Ok(Background {
        target,
        image,
        tiled: request.tiled,
    })
}

fn resolve(image: &str) -> Result<String, PtError> {
    let image = image.trim();
    if image.is_empty() {
        return Ok(String::new());
    }
    if let Some((_, path)) = BUNDLED
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(image))
    {
        return Ok((*path).to_owned());
    }
    if std::path::Path::new(image).is_absolute() {
        return Ok(image.to_owned());
    }
    let names: Vec<&str> = BUNDLED.iter().map(|(name, _)| *name).collect();
    Err(PtError::InvalidInput(format!(
        "`{image}` must be an absolute path or one of Packet Tracer's own backgrounds: {}",
        names.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::absolute;

    #[test]
    fn accepts_bundled_names_and_absolute_paths() {
        assert_eq!(
            resolve("grid_50x50").unwrap(),
            "../art/Background/grid_50x50.png"
        );
        assert_eq!(
            resolve(" CITY ").unwrap(),
            "../art/Background/gGeoViewCity.png"
        );
        let plan = absolute("/tmp/plano.png");
        assert_eq!(resolve(&plan).unwrap(), plan);
        assert_eq!(resolve("").unwrap(), "");
        let error = resolve("plano.png").unwrap_err().to_string();
        assert!(error.contains("absolute path"), "{error}");
    }
}
