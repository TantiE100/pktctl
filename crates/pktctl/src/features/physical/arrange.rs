use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MoveRequest, move_to_location,
    tree::{Node, Snapshot},
};
use crate::packet_tracer::{PacketTracer, PtError};

const DEFAULT_COLUMNS: u32 = 3;
// The physical workspace is measured in metres: a room is hundreds of units wide, so
// devices a few units apart pile up on top of each other.
const DEFAULT_SPACING: i32 = 150;
const DEFAULT_START: i32 = 200;
const MAX_DEVICES: usize = 200;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ArrangeRequest {
    /// Path of the room, building or rack to tidy up, as listed by `list_locations`.
    pub location: String,
    /// Devices to place, in order. Omit to arrange everything already in that location.
    #[serde(default)]
    pub devices: Vec<String>,
    /// How many devices per row. Defaults to 3.
    #[serde(default)]
    pub columns: Option<u32>,
    /// Distance between devices, in the metres the physical workspace uses. Defaults to 150.
    #[serde(default)]
    pub spacing_x: Option<i32>,
    #[serde(default)]
    pub spacing_y: Option<i32>,
    /// Where the first device goes. Defaults to 200, 200.
    #[serde(default)]
    pub start_x: Option<i32>,
    #[serde(default)]
    pub start_y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Placed {
    pub device: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Arrangement {
    pub location: String,
    /// Where each device ended up, row by row.
    pub devices: Vec<Placed>,
}

/// Lays devices out in rows inside one location, moving in the ones that are elsewhere.
pub async fn arrange_devices<P: PacketTracer>(
    packet_tracer: &P,
    request: &ArrangeRequest,
) -> Result<Arrangement, PtError> {
    let columns = request.columns.unwrap_or(DEFAULT_COLUMNS);
    if columns == 0 {
        return Err(PtError::InvalidInput("columns must be at least 1".into()));
    }
    let snapshot = Snapshot::read(packet_tracer).await?;
    let location = snapshot.by_path(&request.location)?.clone();
    let wanted = chosen(&snapshot, &location, &request.devices)?;
    if wanted.len() > MAX_DEVICES {
        return Err(PtError::InvalidInput(format!(
            "arranging more than {MAX_DEVICES} devices at once is not supported"
        )));
    }

    let (spacing_x, spacing_y) = (
        request.spacing_x.unwrap_or(DEFAULT_SPACING),
        request.spacing_y.unwrap_or(DEFAULT_SPACING),
    );
    let (start_x, start_y) = (
        request.start_x.unwrap_or(DEFAULT_START),
        request.start_y.unwrap_or(DEFAULT_START),
    );
    let mut devices = Vec::with_capacity(wanted.len());
    for (index, device) in wanted.into_iter().enumerate() {
        let column = i32::try_from(index % columns as usize).unwrap_or_default();
        let row = i32::try_from(index / columns as usize).unwrap_or_default();
        let (x, y) = (start_x + column * spacing_x, start_y + row * spacing_y);
        move_to_location(
            packet_tracer,
            &MoveRequest {
                device: Some(device.clone()),
                into: location.path.clone(),
                x: Some(x),
                y: Some(y),
                ..MoveRequest::default()
            },
        )
        .await?;
        devices.push(Placed { device, x, y });
    }
    Ok(Arrangement {
        location: location.path,
        devices,
    })
}

fn chosen(snapshot: &Snapshot, location: &Node, asked: &[String]) -> Result<Vec<String>, PtError> {
    if asked.is_empty() {
        let inside = format!("{}/", location.path);
        return Ok(snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.parent.as_deref() == Some(location.path.as_str())
                    || node
                        .parent
                        .as_deref()
                        .is_some_and(|parent| parent.starts_with(&inside) && node.device.is_some())
            })
            .filter_map(|node| node.device.clone())
            .collect());
    }
    asked
        .iter()
        .map(|name| {
            let name = name.trim();
            snapshot.device(name)?;
            Ok(name.to_owned())
        })
        .collect()
}
