use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MoveRequest, move_to_location,
    tree::{Node, Snapshot},
};
use crate::packet_tracer::{PacketTracer, PtError};

const DEFAULT_MARGIN: f64 = 12.0;
const MAX_DEVICES: usize = 200;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ArrangeRequest {
    /// Path of the room, building, rack or table to tidy up, as listed by `list_locations`.
    pub location: String,
    /// Devices to place, in order. Omit to arrange everything already in that location.
    #[serde(default)]
    pub devices: Vec<String>,
    /// How many per row. Defaults to the squarish grid that fits them.
    #[serde(default)]
    pub columns: Option<u32>,
    /// Free space left around the grid, as a percentage of the room. Defaults to 12.
    #[serde(default)]
    pub margin_percent: Option<f64>,
    /// Exact spots instead of a grid, as percentages of the room: `[[20, 30], [60, 30]]`,
    /// one per device.
    #[serde(default)]
    pub spots: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Placed {
    pub device: String,
    /// Where it sits, as a percentage of the room's width and height.
    pub x_percent: f64,
    pub y_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Arrangement {
    pub location: String,
    pub devices: Vec<Placed>,
    /// The temporary copy Packet Tracer has open when the moves went through the network
    /// file, which is what furniture needs; save with `save_network` to keep it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Lays devices out inside one location, moving in the ones that are elsewhere.
pub async fn arrange_devices<P: PacketTracer>(
    packet_tracer: &P,
    request: &ArrangeRequest,
) -> Result<Arrangement, PtError> {
    let snapshot = Snapshot::read(packet_tracer).await?;
    let location = snapshot.by_path(&request.location)?.clone();
    let wanted = chosen(&snapshot, &location, &request.devices)?;
    if wanted.is_empty() {
        return Err(PtError::InvalidInput(format!(
            "`{}` has no devices to arrange",
            location.path
        )));
    }
    if wanted.len() > MAX_DEVICES {
        return Err(PtError::InvalidInput(format!(
            "arranging more than {MAX_DEVICES} devices at once is not supported"
        )));
    }
    let spots = spots(request, wanted.len())?;

    let mut file = None;
    for (node, (x, y)) in wanted.iter().zip(spots.iter().copied()) {
        let moved = move_to_location(
            packet_tracer,
            &MoveRequest {
                device: node.device.clone(),
                location: node.device.is_none().then(|| node.path.clone()),
                into: location.path.clone(),
                x_percent: Some(x),
                y_percent: Some(y),
            },
        )
        .await?;
        file = moved.file.or(file);
    }
    Ok(Arrangement {
        location: location.path,
        devices: wanted
            .iter()
            .zip(spots)
            .map(|(node, (x, y))| Placed {
                device: node.device.clone().unwrap_or_else(|| node.name.clone()),
                x_percent: x,
                y_percent: y,
            })
            .collect(),
        file,
    })
}

/// Percentages for each device: the ones asked for, or a grid with room to breathe.
fn spots(request: &ArrangeRequest, count: usize) -> Result<Vec<(f64, f64)>, PtError> {
    if !request.spots.is_empty() {
        if request.spots.len() != count {
            return Err(PtError::InvalidInput(format!(
                "there are {count} devices and {} spots",
                request.spots.len()
            )));
        }
        if request
            .spots
            .iter()
            .flatten()
            .any(|percent| !(0.0..=100.0).contains(percent))
        {
            return Err(PtError::InvalidInput(
                "spots are percentages of the room, between 0 and 100".into(),
            ));
        }
        return Ok(request.spots.iter().map(|[x, y]| (*x, *y)).collect());
    }

    let margin = request.margin_percent.unwrap_or(DEFAULT_MARGIN);
    if !(0.0..45.0).contains(&margin) {
        return Err(PtError::InvalidInput(
            "margin_percent must be between 0 and 45".into(),
        ));
    }
    let columns = match request.columns {
        Some(0) => return Err(PtError::InvalidInput("columns must be at least 1".into())),
        Some(columns) => columns as usize,
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        None => (count as f64).sqrt().ceil() as usize,
    };
    let rows = count.div_ceil(columns);
    let step = |index: usize, total: usize| {
        if total <= 1 {
            50.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let fraction = index as f64 / (total - 1) as f64;
            margin + (100.0 - 2.0 * margin) * fraction
        }
    };
    Ok((0..count)
        .map(|index| {
            (
                step(index % columns, columns.min(count)),
                step(index / columns, rows),
            )
        })
        .collect())
}

fn chosen(snapshot: &Snapshot, location: &Node, asked: &[String]) -> Result<Vec<Node>, PtError> {
    if asked.is_empty() {
        return Ok(snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.device.is_some() && node.parent.as_deref() == Some(location.path.as_str())
            })
            .cloned()
            .collect());
    }
    asked
        .iter()
        .map(|name| snapshot.device(name.trim()).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(count: usize, columns: Option<u32>) -> Vec<(f64, f64)> {
        let request = ArrangeRequest {
            location: "Home City".into(),
            columns,
            ..ArrangeRequest::default()
        };
        spots(&request, count).unwrap()
    }

    #[test]
    fn spreads_devices_across_the_room() {
        assert_eq!(
            grid(4, Some(2)),
            [(12.0, 12.0), (88.0, 12.0), (12.0, 88.0), (88.0, 88.0)]
        );
        assert_eq!(grid(1, None), [(50.0, 50.0)]);

        let nine = grid(9, None);
        assert_eq!(nine.len(), 9);
        assert_eq!(
            nine[4],
            (50.0, 50.0),
            "the middle device sits in the middle"
        );
        assert!(
            nine.iter()
                .all(|(x, y)| (12.0..=88.0).contains(x) && (12.0..=88.0).contains(y))
        );
    }

    #[test]
    fn takes_exact_spots_and_checks_them() {
        let request = ArrangeRequest {
            location: "Home City".into(),
            devices: vec!["PC1".into(), "PC2".into()],
            spots: vec![[10.0, 20.0], [80.0, 90.0]],
            ..ArrangeRequest::default()
        };
        assert_eq!(spots(&request, 2).unwrap(), [(10.0, 20.0), (80.0, 90.0)]);
        assert!(spots(&request, 3).is_err(), "one spot per device");

        let outside = ArrangeRequest {
            spots: vec![[10.0, 120.0]],
            ..request
        };
        assert!(spots(&outside, 1).is_err());
    }
}
