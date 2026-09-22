use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    file_edit::{DEFAULT_POSITION, FileEdit, add_node, move_node},
    tree::{Node, Snapshot, is_duplicate, object, plain_name, split},
};
use crate::{
    features::paths::{app_window, device},
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_text},
};

const MAX_CLIMB: usize = 12;
/// Packet Tracer draws the contents of a container across a scene of this size and keeps
/// each device's position as a fraction of it, so positions are given as percentages.
pub(crate) const SCENE: (f64, f64) = (3444.0, 2157.0);
const CONTAINER_KINDS_FOR_CLOSETS: &[&str] = &["universe", "city", "building"];
/// Where Packet Tracer puts a device dropped into a wiring closet: the rack of the
/// default closets, the table of new ones.
const FURNITURE: &[&str] = &[
    "rack",
    "stackable_table",
    "old_table",
    "shelf",
    "cable_pegboard",
    "generic_container",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NewLocation {
    /// A city in Intercity.
    #[default]
    City,
    /// A wiring closet in Intercity, a city or a building.
    WiringCloset,
    /// A building in a city.
    Building,
    /// A rack, for devices mounted in a wiring closet.
    Rack,
    /// A table to lay devices on.
    Table,
    /// A shelf.
    Shelf,
    /// A cable pegboard.
    CablePegboard,
    /// A generic container, for anything else.
    Container,
}

impl NewLocation {
    /// `PhysicalObjectType` value, for the kinds written straight into the network file.
    pub(crate) fn type_code(self) -> Option<i64> {
        Some(match self {
            Self::City | Self::WiringCloset => return None,
            Self::Building => 2,
            Self::Rack => 4,
            Self::Container => 8,
            Self::Shelf => 9,
            Self::Table => 10,
            Self::CablePegboard => 11,
        })
    }

    pub(crate) fn default_name(self) -> &'static str {
        match self {
            Self::City => "City",
            Self::WiringCloset => "Wiring Closet",
            Self::Building => "Building",
            Self::Rack => "Rack",
            Self::Table => "Table",
            Self::Shelf => "Shelf",
            Self::CablePegboard => "Cable Pegboard",
            Self::Container => "Container",
        }
    }

    /// Where Packet Tracer accepts this kind.
    pub(crate) fn goes_inside(self) -> &'static [&'static str] {
        match self {
            Self::City => &["universe"],
            Self::WiringCloset => CONTAINER_KINDS_FOR_CLOSETS,
            Self::Building => &["city"],
            _ => &["wiring_closet", "building", "city", "generic_container"],
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AddLocationRequest {
    /// What to create: `city`, `wiring_closet`, `building`, `rack`, `table`, `shelf`,
    /// `cable_pegboard` or `container`. Packet Tracer's own API only creates cities and
    /// wiring closets; the rest are written into the network file.
    pub kind: NewLocation,
    /// Name for the new location. Packet Tracer's default name is used when omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// Path of the city or building to put a wiring closet in, for example
    /// `Home City/Corporate Office`. Omit for Intercity. Cities always go in Intercity.
    #[serde(default)]
    pub inside: Option<String>,
    /// Where to put it inside its parent, as a percentage of the room's width and height.
    #[serde(default)]
    pub x_percent: Option<f64>,
    #[serde(default)]
    pub y_percent: Option<f64>,
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
    /// Devices moved into a wiring closet land on its rack or table, as Packet Tracer
    /// places them.
    pub into: String,
    /// Where to leave it inside the destination, as a percentage of the room's width and
    /// height: 50 and 50 is the middle.
    #[serde(default)]
    pub x_percent: Option<f64>,
    #[serde(default)]
    pub y_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Moved {
    /// The device name or location name that moved.
    pub moved: String,
    /// Path of the location it ended up in.
    pub now_in: String,
    /// The temporary copy Packet Tracer now has open, when the move went through the
    /// network file. Save with `save_network` and a path to keep it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

pub async fn add_location<P: PacketTracer>(
    packet_tracer: &P,
    request: &AddLocationRequest,
) -> Result<FileEdit, PtError> {
    let before = Snapshot::read(packet_tracer).await?;
    let target = match request.inside.as_deref().map(str::trim) {
        None | Some("") => before.by_path("")?,
        Some(path) => before.by_path(path)?,
    };
    let button = match request.kind {
        NewLocation::City => "addCity",
        _ => "addCloset",
    };
    if !request.kind.goes_inside().contains(&target.kind.as_str()) {
        return Err(PtError::InvalidInput(format!(
            "a {} cannot go inside `{}`, which is a {}",
            label(request.kind),
            target.name,
            target.kind
        )));
    }
    ensure_reachable(&target.path)?;
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(kind) = request.kind.type_code() {
        let position = match percent_spot(request.x_percent, request.y_percent)? {
            Some(spot) => spot,
            None => DEFAULT_POSITION,
        };
        let target_path = target.path.clone();
        return add_node(
            packet_tracer,
            kind,
            &target_path,
            name.unwrap_or_else(|| request.kind.default_name()),
            position,
        )
        .await;
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
    place_at(
        packet_tracer,
        &handle,
        percent_spot(request.x_percent, request.y_percent)?,
    )
    .await?;
    if let Some(name) = name {
        packet_tracer
            .call(handle.clone().method("setName", [Value::qstring(name)]))
            .await?;
    }
    let location = Snapshot::read(packet_tracer).await?.location(&created)?;
    Ok(FileEdit {
        location,
        file: None,
    })
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
    ensure_reachable(&target.path)?;
    if !subject.is_device()
        && (target.path == subject.path || target.path.starts_with(&format!("{}/", subject.path)))
    {
        return Err(PtError::InvalidInput(format!(
            "`{}` cannot move inside itself",
            subject.path
        )));
    }

    if FURNITURE.contains(&target.kind.as_str()) {
        return move_node(
            packet_tracer,
            &subject,
            &target,
            percent_spot(request.x_percent, request.y_percent)?,
        )
        .await;
    }

    let handle = match &subject.device {
        Some(name) => device(name).method("getPhysicalObject", []),
        None => object(&subject.uuid),
    };
    let current_parent = climb(packet_tracer, &snapshot, &handle, &subject, &target.path).await?;
    descend(packet_tracer, &handle, &current_parent, &target.path).await?;
    place_at(
        packet_tracer,
        &handle,
        percent_spot(request.x_percent, request.y_percent)?,
    )
    .await?;

    let finished = Snapshot::read(packet_tracer).await?;
    let moved = match &subject.device {
        Some(name) => finished.device(name)?,
        None => finished.by_uuid(&subject.uuid).ok_or_else(|| {
            PtError::UnexpectedReply(format!("`{}` vanished while moving", subject.name))
        })?,
    };
    let now = moved.parent.clone().unwrap_or_default();
    let landed = now == target.path
        || finished.by_path(&now).is_ok_and(|node| {
            FURNITURE.contains(&node.kind.as_str()) && node.parent.as_deref() == Some(&target.path)
        });
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
        file: None,
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

/// Packet Tracer moves things only into the first of several same-named locations, so a
/// path through a `Name#2` segment cannot be reached; checked before anything changes.
fn ensure_reachable(target: &str) -> Result<(), PtError> {
    match split(target)
        .into_iter()
        .find(|segment| is_duplicate(segment))
    {
        Some(segment) => Err(PtError::InvalidInput(format!(
            "`{segment}` shares its name with another location at the same level; Packet \
             Tracer only moves things into the first one, so give the locations distinct names \
             first"
        ))),
        None => Ok(()),
    }
}

async fn descend<P: PacketTracer>(
    packet_tracer: &P,
    handle: &Call,
    from: &str,
    target: &str,
) -> Result<(), PtError> {
    let from_depth = split(from).len();
    let mut level = from.to_owned();
    ensure_reachable(target)?;
    for segment in split(target).into_iter().skip(from_depth) {
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

/// Turns percentages of the room into the coordinates Packet Tracer stores.
pub(crate) fn percent_spot(x: Option<f64>, y: Option<f64>) -> Result<Option<(i32, i32)>, PtError> {
    let (Some(x), Some(y)) = (x, y) else {
        if x.is_some() || y.is_some() {
            return Err(PtError::InvalidInput(
                "give both x_percent and y_percent, or neither".into(),
            ));
        }
        return Ok(None);
    };
    if !(0.0..=100.0).contains(&x) || !(0.0..=100.0).contains(&y) {
        return Err(PtError::InvalidInput(
            "x_percent and y_percent are percentages of the room, between 0 and 100".into(),
        ));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(Some((
        (x / 100.0 * SCENE.0).round() as i32,
        (y / 100.0 * SCENE.1).round() as i32,
    )))
}

async fn place_at<P: PacketTracer>(
    packet_tracer: &P,
    handle: &Call,
    spot: Option<(i32, i32)>,
) -> Result<(), PtError> {
    let Some((x, y)) = spot else {
        return Ok(());
    };
    packet_tracer
        .call(
            handle
                .clone()
                .method("moveTo", [Value::Int(x), Value::Int(y)]),
        )
        .await
        .map(drop)
}

fn label(kind: NewLocation) -> &'static str {
    match kind {
        NewLocation::City => "city",
        NewLocation::WiringCloset => "wiring closet",
        NewLocation::Building => "building",
        NewLocation::Rack => "rack",
        NewLocation::Table => "table",
        NewLocation::Shelf => "shelf",
        NewLocation::CablePegboard => "cable pegboard",
        NewLocation::Container => "container",
    }
}
