use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::tree::{Location, Node, Snapshot, is_duplicate, object, plain_name, split};
use crate::{
    features::paths::{app_window, device},
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_text},
};

const MAX_CLIMB: usize = 12;
const CONTAINER_KINDS_FOR_CLOSETS: &[&str] = &["universe", "city", "building"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NewLocation {
    #[default]
    City,
    WiringCloset,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AddLocationRequest {
    /// What to create. Packet Tracer only lets external apps create cities and wiring closets.
    pub kind: NewLocation,
    /// Path of the city or building to put a wiring closet in, for example
    /// `Home City/Corporate Office`. Omit for Intercity. Cities always go in Intercity.
    #[serde(default)]
    pub inside: Option<String>,
    /// Optional position inside its parent.
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MoveRequest {
    /// Device to move, by its name in `list_devices`. Give this or `location`.
    #[serde(default)]
    pub device: Option<String>,
    /// Location to move, by its path in `list_locations`. Give this or `device`.
    #[serde(default)]
    pub location: Option<String>,
    /// Path of the destination, for example `Home City/Corporate Office/Main Wiring Closet`.
    /// Devices moved into a wiring closet are mounted in its rack.
    pub into: String,
    /// Optional position inside the destination.
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Moved {
    /// The device name or location name that moved.
    pub moved: String,
    /// Path of the location it ended up in.
    pub now_in: String,
}

pub async fn add_location<P: PacketTracer>(
    packet_tracer: &P,
    request: &AddLocationRequest,
) -> Result<Location, PtError> {
    let before = Snapshot::read(packet_tracer).await?;
    let target = match request.inside.as_deref().map(str::trim) {
        None | Some("") => before.by_path("")?,
        Some(path) => before.by_path(path)?,
    };
    let (button, allowed): (&str, &[&str]) = match request.kind {
        NewLocation::City => ("addCity", &["universe"]),
        NewLocation::WiringCloset => ("addCloset", CONTAINER_KINDS_FOR_CLOSETS),
    };
    if !allowed.contains(&target.kind.as_str()) {
        return Err(PtError::InvalidInput(format!(
            "a {} cannot go inside `{}`, which is a {}",
            label(request.kind),
            target.name,
            target.kind
        )));
    }
    let existing: Vec<String> = before.children("").map(|node| node.uuid.clone()).collect();

    let toolbar = app_window().method("getPhysicalToolbar", []);
    packet_tracer
        .call(toolbar.clone().method("switchToTopView", []))
        .await?;
    packet_tracer.call(toolbar.method(button, [])).await?;

    let after = Snapshot::read(packet_tracer).await?;
    let created = after
        .children("")
        .find(|node| !existing.contains(&node.uuid))
        .ok_or_else(|| {
            PtError::Rejected(format!(
                "Packet Tracer did not create the {}",
                label(request.kind)
            ))
        })?
        .uuid
        .clone();

    let target_path = target.path.clone();
    let handle = object(&created);
    descend(packet_tracer, &handle, "", &target_path).await?;
    place_at(packet_tracer, &handle, request.x, request.y).await?;
    Snapshot::read(packet_tracer).await?.location(&created)
}

pub async fn move_to_location<P: PacketTracer>(
    packet_tracer: &P,
    request: &MoveRequest,
) -> Result<Moved, PtError> {
    let snapshot = Snapshot::read(packet_tracer).await?;
    let subject = match (request.device.as_deref(), request.location.as_deref()) {
        (Some(name), None) => snapshot.device(name.trim())?,
        (None, Some(path)) => {
            let node = snapshot.by_path(path)?;
            if node.path.is_empty() {
                return Err(PtError::InvalidInput("Intercity cannot be moved".into()));
            }
            node
        }
        _ => {
            return Err(PtError::InvalidInput(
                "give exactly one of `device` or `location`".into(),
            ));
        }
    }
    .clone();
    let target = snapshot.by_path(&request.into)?.clone();
    if !subject.is_device()
        && (target.path == subject.path || target.path.starts_with(&format!("{}/", subject.path)))
    {
        return Err(PtError::InvalidInput(format!(
            "`{}` cannot move inside itself",
            subject.path
        )));
    }

    let handle = match &subject.device {
        Some(name) => device(name).method("getPhysicalObject", []),
        None => object(&subject.uuid),
    };
    let current_parent = climb(packet_tracer, &snapshot, &handle, &subject, &target.path).await?;
    descend(packet_tracer, &handle, &current_parent, &target.path).await?;
    place_at(packet_tracer, &handle, request.x, request.y).await?;

    let finished = Snapshot::read(packet_tracer).await?;
    let moved = match &subject.device {
        Some(name) => finished.device(name)?,
        None => finished.by_uuid(&subject.uuid).ok_or_else(|| {
            PtError::UnexpectedReply(format!("`{}` vanished while moving", subject.name))
        })?,
    };
    let now = moved.parent.clone().unwrap_or_default();
    let landed = now == target.path
        || finished
            .by_path(&now)
            .is_ok_and(|node| node.kind == "rack" && node.parent.as_deref() == Some(&target.path));
    if !landed {
        return Err(PtError::Rejected(format!(
            "Packet Tracer left `{}` in `{now}` instead of `{}`",
            subject.device.as_deref().unwrap_or(&subject.name),
            target.path
        )));
    }
    Ok(Moved {
        moved: subject.device.clone().unwrap_or(subject.name),
        now_in: now,
    })
}

async fn climb<P: PacketTracer>(
    packet_tracer: &P,
    snapshot: &Snapshot,
    handle: &Call,
    subject: &Node,
    target: &str,
) -> Result<String, PtError> {
    let target_segments = split(target);
    let mut parent = subject.parent.clone().unwrap_or_default();
    for _ in 0..MAX_CLIMB {
        let parent_segments = split(&parent);
        let shared = parent_segments
            .iter()
            .zip(&target_segments)
            .take_while(|(left, right)| left == right)
            .count();
        if shared == parent_segments.len() {
            return Ok(parent);
        }
        let moved = packet_tracer
            .call(handle.clone().method("moveOutOfCurrentObject", []))
            .await?;
        if !expect_bool(&moved, "moveOutOfCurrentObject result")? {
            return Err(PtError::Rejected(format!(
                "Packet Tracer would not move `{}` out of `{parent}`",
                subject.name
            )));
        }
        let uuid = packet_tracer
            .call(
                handle
                    .clone()
                    .method("getParent", [])
                    .method("getObjectUuid", []),
            )
            .await?;
        let uuid = expect_text(&uuid, "parent uuid")?;
        parent = snapshot
            .by_uuid(&uuid)
            .map(|node| node.path.clone())
            .ok_or_else(|| PtError::UnexpectedReply("unknown parent after moving out".into()))?;
    }
    Err(PtError::Rejected(format!(
        "could not bring `{}` up to `{target}`",
        subject.name
    )))
}

async fn descend<P: PacketTracer>(
    packet_tracer: &P,
    handle: &Call,
    from: &str,
    target: &str,
) -> Result<(), PtError> {
    let from_depth = split(from).len();
    let mut level = from.to_owned();
    for segment in split(target).into_iter().skip(from_depth) {
        if is_duplicate(segment) {
            return Err(PtError::InvalidInput(format!(
                "`{segment}` shares its name with another location at the same level; Packet \
                 Tracer only moves things into the first one, so give the locations distinct \
                 names first"
            )));
        }
        let moved = packet_tracer
            .call(
                handle
                    .clone()
                    .method("moveIntoObject", [Value::qstring(plain_name(segment))]),
            )
            .await?;
        if !expect_bool(&moved, "moveIntoObject result")? {
            return Err(PtError::Rejected(format!(
                "Packet Tracer would not move into `{segment}` from `{}`",
                if level.is_empty() {
                    "Intercity"
                } else {
                    &level
                }
            )));
        }
        level = if level.is_empty() {
            segment.to_owned()
        } else {
            format!("{level}/{segment}")
        };
    }
    Ok(())
}

async fn place_at<P: PacketTracer>(
    packet_tracer: &P,
    handle: &Call,
    x: Option<i32>,
    y: Option<i32>,
) -> Result<(), PtError> {
    match (x, y) {
        (Some(x), Some(y)) => packet_tracer
            .call(
                handle
                    .clone()
                    .method("moveTo", [Value::Int(x), Value::Int(y)]),
            )
            .await
            .map(drop),
        (None, None) => Ok(()),
        _ => Err(PtError::InvalidInput(
            "give both x and y, or neither".into(),
        )),
    }
}

fn label(kind: NewLocation) -> &'static str {
    match kind {
        NewLocation::City => "city",
        NewLocation::WiringCloset => "wiring closet",
    }
}
