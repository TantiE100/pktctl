use futures::future::try_join_all;
use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{links::list_links, paths::logical_workspace},
    packet_tracer::{
        PacketTracer, PtError, expect_bool, expect_integer, expect_number, expect_text,
    },
};

const PORT_ABBREVIATIONS: &[(&str, &str)] = &[
    ("GigabitEthernet", "Gig"),
    ("FastEthernet", "Fa"),
    ("Serial", "Se"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NoteRequest {
    /// Canvas x coordinate of the note.
    pub x: i32,
    /// Canvas y coordinate of the note.
    pub y: i32,
    /// Text to show, for example `LAN 192.168.10.0/24 (VLAN 10)`.
    pub text: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NotesRequest {
    /// Also list the port labels Packet Tracer draws at each end of a cable (`Gig0/0`, `Fa0/1`).
    #[serde(default)]
    pub include_port_labels: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NoteRef {
    /// Note id as returned by `add_note` or `list_notes`.
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NoteList {
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NoteRemoved {
    pub removed: String,
}

pub async fn add_note<P: PacketTracer>(
    packet_tracer: &P,
    request: &NoteRequest,
) -> Result<Note, PtError> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err(PtError::InvalidInput("a note needs text".into()));
    }
    let layer = packet_tracer
        .call(logical_workspace().method("getIncNoteZOrder", []))
        .await?;
    let layer = expect_number(&layer, "note layer")?;
    let id = packet_tracer
        .call(logical_workspace().method(
            "addNote",
            [
                Value::Int(request.x),
                Value::Int(request.y),
                Value::Double(layer),
                Value::qstring(text),
            ],
        ))
        .await?;
    read_note(packet_tracer, expect_text(&id, "note id")?).await
}

pub async fn list_notes<P: PacketTracer>(
    packet_tracer: &P,
    request: &NotesRequest,
) -> Result<NoteList, PtError> {
    let labels = if request.include_port_labels {
        Vec::new()
    } else {
        port_labels(packet_tracer).await?
    };
    let ids = packet_tracer
        .call(logical_workspace().method("getCanvasNoteIds", []))
        .await?
        .into_items()
        .ok_or_else(|| PtError::UnexpectedReply("note ids should be a list".into()))?;
    let notes = try_join_all(
        ids.iter()
            .map(|id| async move { read_note(packet_tracer, expect_text(id, "note id")?).await }),
    )
    .await?;
    Ok(NoteList {
        notes: notes
            .into_iter()
            .filter(|note| !labels.contains(&note.text))
            .collect(),
    })
}

async fn port_labels<P: PacketTracer>(packet_tracer: &P) -> Result<Vec<String>, PtError> {
    Ok(list_links(packet_tracer)
        .await?
        .links
        .iter()
        .flat_map(|link| [&link.a.port, &link.b.port])
        .map(|port| abbreviate(port))
        .collect())
}

fn abbreviate(port: &str) -> String {
    PORT_ABBREVIATIONS
        .iter()
        .find_map(|(full, short)| port.strip_prefix(full).map(|rest| format!("{short}{rest}")))
        .unwrap_or_else(|| port.to_owned())
}

pub async fn remove_note<P: PacketTracer>(
    packet_tracer: &P,
    request: &NoteRef,
) -> Result<NoteRemoved, PtError> {
    let id = request.id.trim();
    let removed = packet_tracer
        .call(logical_workspace().method("removeCanvasItem", [Value::Uuid(id.to_owned())]))
        .await?;
    if !expect_bool(&removed, "removeCanvasItem result")? {
        return Err(PtError::NotFound(format!("note `{id}`")));
    }
    Ok(NoteRemoved {
        removed: id.to_owned(),
    })
}

async fn read_note<P: PacketTracer>(packet_tracer: &P, id: String) -> Result<Note, PtError> {
    let item = || Value::Uuid(id.clone());
    let (text, x, y) = tokio::try_join!(
        packet_tracer.call(logical_workspace().method("getCanvasNoteText", [item()])),
        packet_tracer.call(logical_workspace().method("getCanvasItemRealX", [item()])),
        packet_tracer.call(logical_workspace().method("getCanvasItemRealY", [item()])),
    )?;
    Ok(Note {
        text: expect_text(&text, "note text")?,
        x: expect_integer(&x, "note x")?,
        y: expect_integer(&y, "note y")?,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_ports_like_packet_tracer_labels() {
        assert_eq!(abbreviate("GigabitEthernet0/0"), "Gig0/0");
        assert_eq!(abbreviate("FastEthernet0/1"), "Fa0/1");
        assert_eq!(abbreviate("Serial0/0/1"), "Se0/0/1");
        assert_eq!(abbreviate("Vlan1"), "Vlan1");
    }
}
