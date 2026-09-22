use std::path::Path;

use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{
        devices::count,
        paths::{app_window, system_files},
    },
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_integer, expect_text},
};

const EXTENSIONS: &[&str] = &["pkt", "pka"];
const FILE_OPEN_ERRORS: &[(i64, &str)] = &[
    (1, "the file's SSL signature is invalid"),
    (2, "the file could not be decompressed"),
    (3, "the file is not a valid Packet Tracer binary"),
    (4, "the file's metadata is invalid"),
    (5, "the file's configuration is invalid"),
    (6, "the file could not be read"),
    (7, "the file's options are invalid"),
    (8, "opening was cancelled"),
    (9, "the file has an unexpected format"),
    (10, "Packet Tracer ran out of resources"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SaveRequest {
    /// Absolute path ending in `.pkt`. Omit it to save over the file already open.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct OpenRequest {
    /// Absolute path of a `.pkt` or `.pka` file.
    pub path: String,
    /// Save the current network here first. Without it, unsaved changes are discarded.
    #[serde(default)]
    pub save_current_to: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NewRequest {
    /// Save the current network here first. Without it, unsaved changes are discarded.
    #[serde(default)]
    pub save_current_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Saved {
    pub path: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Opened {
    pub path: String,
    pub devices: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_previous: Option<Saved>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Cleared {
    pub cleared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_previous: Option<Saved>,
}

pub async fn save<P: PacketTracer>(
    packet_tracer: &P,
    request: &SaveRequest,
) -> Result<Saved, PtError> {
    let path = match request.path.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => network_path(path)?,
        _ => current_file(packet_tracer).await?,
    };
    save_to(packet_tracer, &path).await
}

pub async fn open<P: PacketTracer>(
    packet_tracer: &P,
    request: &OpenRequest,
) -> Result<Opened, PtError> {
    let path = network_path(request.path.trim())?;
    if !file_exists(packet_tracer, &path).await? {
        return Err(PtError::NotFound(format!("file `{path}`")));
    }
    let saved_previous = save_current(packet_tracer, request.save_current_to.as_deref()).await?;
    clear(packet_tracer).await?;

    let code = packet_tracer
        .call(app_window().method("fileOpen", [Value::qstring(&path)]))
        .await?;
    let code = expect_integer(&code, "fileOpen result")?;
    if code != 0 {
        let reason = FILE_OPEN_ERRORS
            .iter()
            .find(|(value, _)| *value == code)
            .map_or("Packet Tracer reported an unknown error", |(_, reason)| {
                reason
            });
        return Err(PtError::Rejected(format!(
            "could not open `{path}`: {reason}"
        )));
    }
    Ok(Opened {
        devices: count(packet_tracer).await?,
        path,
        saved_previous,
    })
}

pub async fn new_network<P: PacketTracer>(
    packet_tracer: &P,
    request: &NewRequest,
) -> Result<Cleared, PtError> {
    let saved_previous = save_current(packet_tracer, request.save_current_to.as_deref()).await?;
    clear(packet_tracer).await?;
    Ok(Cleared {
        cleared: true,
        saved_previous,
    })
}

async fn save_current<P: PacketTracer>(
    packet_tracer: &P,
    target: Option<&str>,
) -> Result<Option<Saved>, PtError> {
    match target.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => Ok(Some(save_to(packet_tracer, &network_path(path)?).await?)),
        None => Ok(None),
    }
}

async fn save_to<P: PacketTracer>(packet_tracer: &P, path: &str) -> Result<Saved, PtError> {
    packet_tracer
        .call(app_window().method(
            "fileSaveAsNoPrompt",
            [Value::qstring(path), Value::Bool(false)],
        ))
        .await?;
    if !file_exists(packet_tracer, path).await? {
        return Err(PtError::Rejected(format!(
            "Packet Tracer did not write `{path}`; check that the folder exists and is writable"
        )));
    }
    let bytes = packet_tracer
        .call(system_files().method("getFileSize", [Value::qstring(path)]))
        .await?;
    Ok(Saved {
        path: path.to_owned(),
        bytes: expect_integer(&bytes, "file size")?,
    })
}

async fn clear<P: PacketTracer>(packet_tracer: &P) -> Result<(), PtError> {
    let created = packet_tracer
        .call(app_window().method("fileNew", [Value::Bool(false)]))
        .await?;
    if expect_bool(&created, "fileNew result")? {
        Ok(())
    } else {
        Err(PtError::Rejected(
            "Packet Tracer did not start a new network".into(),
        ))
    }
}

async fn current_file<P: PacketTracer>(packet_tracer: &P) -> Result<String, PtError> {
    let name = packet_tracer
        .call(
            app_window()
                .method("getActiveFile", [])
                .method("getSavedFilename", []),
        )
        .await?;
    let name = expect_text(&name, "current file name")?;
    if name.trim().is_empty() {
        return Err(PtError::InvalidInput(
            "this network has never been saved; give a path".into(),
        ));
    }
    Ok(name)
}

async fn file_exists<P: PacketTracer>(packet_tracer: &P, path: &str) -> Result<bool, PtError> {
    let exists = packet_tracer
        .call(system_files().method("fileExists", [Value::qstring(path)]))
        .await?;
    expect_bool(&exists, "fileExists result")
}

fn network_path(path: &str) -> Result<String, PtError> {
    let candidate = Path::new(path);
    if !candidate.is_absolute() {
        return Err(PtError::InvalidInput(format!(
            "`{path}` must be an absolute path"
        )));
    }
    let extension = candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension {
        Some(extension) if EXTENSIONS.contains(&extension.as_str()) => Ok(path.to_owned()),
        _ => Err(PtError::InvalidInput(format!(
            "`{path}` must end in .pkt or .pka"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_absolute_packet_tracer_files() {
        assert!(network_path("/tmp/lab.pkt").is_ok());
        assert!(network_path("/tmp/activity.PKA").is_ok());
        assert!(network_path("lab.pkt").is_err());
        assert!(network_path("/tmp/lab.txt").is_err());
        assert!(network_path("/tmp/lab").is_err());
    }
}
