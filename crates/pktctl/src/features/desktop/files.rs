use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::App;
use crate::packet_tracer::{PacketTracer, PtError, expect_bool, expect_integer, expect_text};

const PROCESS: &str = "FileManager";
const ROOT: &str = "c:/";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    /// Every file in C:\ with its size.
    #[default]
    List,
    /// The text of `name`.
    Read,
    /// Create `name` with `text`, or replace its text.
    Write,
    /// Remove `name`.
    Delete,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FilesRequest {
    /// PC, laptop or server.
    pub device: String,
    /// `list` (default), `read`, `write` or `delete`.
    #[serde(default)]
    pub action: FileAction,
    /// File name in C:\, for example `notes.txt`. Required except for `list`.
    #[serde(default)]
    pub name: Option<String>,
    /// Text to write. Required for `write`.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileEntry {
    pub name: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FilesResult {
    pub device: String,
    pub action: FileAction,
    /// The files in C:\ after the action.
    pub files: Vec<FileEntry>,
    /// The file's text, for `read`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub async fn host_files<P: PacketTracer>(
    packet_tracer: &P,
    request: &FilesRequest,
) -> Result<FilesResult, PtError> {
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let name = match (request.action, name) {
        (FileAction::List, _) => None,
        (_, Some(name)) if name.contains(['/', '\\']) => {
            return Err(PtError::InvalidInput(format!(
                "`{name}`: give a file name in C:\\, without folders"
            )));
        }
        (_, Some(name)) => Some(name),
        (_, None) => {
            return Err(PtError::InvalidInput(
                "name is required to read, write or delete a file".into(),
            ));
        }
    };
    if request.action == FileAction::Write && request.text.is_none() {
        return Err(PtError::InvalidInput(
            "text is required to write a file".into(),
        ));
    }

    let app = App::open(packet_tracer, &request.device, PROCESS, "file system").await?;
    let root = app.at("getDirectory", [Value::string(ROOT), Value::Bool(false)]);
    let mut text = None;
    if let Some(name) = name {
        let exists = app
            .call(root.clone().method("fileExist", [Value::string(name)]))
            .await?;
        let exists = expect_bool(&exists, "fileExist")?;
        let file = root.clone().method("getFile", [Value::string(name)]);
        match (request.action, exists) {
            (FileAction::Read | FileAction::Delete, false) => {
                return Err(PtError::NotFound(format!(
                    "file `{name}` on `{}`",
                    app.device
                )));
            }
            (FileAction::Read, true) => text = Some(read(&app, file).await?),
            (FileAction::Write, true) => {
                app.call(file.method(
                    "setTextContent",
                    [
                        Value::string(request.text.as_deref().unwrap_or_default()),
                        Value::Bool(false),
                    ],
                ))
                .await?;
            }
            (FileAction::Write, false) => {
                let added = app
                    .call(root.clone().method(
                        "addTextFile",
                        [
                            Value::string(name),
                            Value::string(request.text.as_deref().unwrap_or_default()),
                            Value::Bool(false),
                        ],
                    ))
                    .await?;
                if !expect_bool(&added, "addTextFile")? {
                    return Err(PtError::Rejected(format!("`{name}` could not be created")));
                }
            }
            (FileAction::Delete, true) => {
                let removed = app
                    .call(
                        root.clone()
                            .method("removeFile", [Value::string(name), Value::Bool(false)]),
                    )
                    .await?;
                if !expect_bool(&removed, "removeFile")? {
                    return Err(PtError::Rejected(format!("`{name}` could not be deleted")));
                }
            }
            (FileAction::List, _) => {}
        }
    }
    Ok(FilesResult {
        files: list(&app, &root).await?,
        device: app.device,
        action: request.action,
        text,
    })
}

async fn read<P: PacketTracer>(app: &App<'_, P>, file: Call) -> Result<String, PtError> {
    match app
        .call(file.method("getContent", [Value::Bool(false)]))
        .await?
    {
        Value::Data { fields, .. } => fields
            .first()
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| PtError::UnexpectedReply("the file has no text content".into())),
        other => Err(PtError::UnexpectedReply(format!(
            "getContent should return the file content, got {other:?}"
        ))),
    }
}

async fn list<P: PacketTracer>(app: &App<'_, P>, root: &Call) -> Result<Vec<FileEntry>, PtError> {
    let count = app.call(root.clone().method("getFileCount", [])).await?;
    let count = i32::try_from(expect_integer(&count, "file count")?)
        .map_err(|_| PtError::UnexpectedReply("file count out of range".into()))?;
    let mut files = Vec::new();
    for index in 0..count {
        let file = root.clone().method("getFileAt", [Value::Int(index)]);
        let (name, size) = tokio::try_join!(
            app.call(file.clone().method("getName", [])),
            app.call(file.method("getSize", [])),
        )?;
        files.push(FileEntry {
            name: expect_text(&name, "file name")?,
            size: expect_integer(&size, "file size")?,
        });
    }
    Ok(files)
}
