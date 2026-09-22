use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::tree::{Location, Snapshot};
use crate::{
    features::{
        devices::{list, remove},
        network_file::{PDU_MODEL, edit_saved_network, file_error},
    },
    packet_tracer::{PacketTracer, PtError},
};

const DEFAULT_BUILDING_POSITION: (i32, i32) = (100, 100);

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RenameLocationRequest {
    /// Path of the location, as listed by `list_locations`.
    pub path: String,
    /// New name, for example `Edificio Central`.
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AddBuildingRequest {
    /// Path of the city the building goes in, for example `Home City`.
    pub inside: String,
    /// Name of the building, for example `Alcaldía`.
    pub name: String,
    /// Position inside the city. Defaults to 100, 100.
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RemoveLocationRequest {
    /// Path of the city, building, wiring closet or rack to delete, as listed by
    /// `list_locations`. Everything inside it goes too.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LocationRemoved {
    pub removed: String,
    /// Power Distribution Devices that were inside and were removed with it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub power_units_removed: Vec<String>,
    /// The temporary copy Packet Tracer now has open. Your own file is untouched: save
    /// with `save_network` and a path to keep the change there.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileEdit {
    pub location: Location,
    /// The temporary copy Packet Tracer now has open. Your own file is untouched: save
    /// with `save_network` and a path to keep the change there.
    pub file: String,
}

pub async fn rename_location<P: PacketTracer>(
    packet_tracer: &P,
    request: &RenameLocationRequest,
) -> Result<FileEdit, PtError> {
    let name = checked_name(&request.name)?;
    let snapshot = Snapshot::read(packet_tracer).await?;
    let node = snapshot.by_path(&request.path)?;
    let id = node.persistent.clone();
    let file = edit_saved_network(packet_tracer, |xml| {
        pktfile::rename_node(xml, &id, name).map_err(|error| file_error(&error))
    })
    .await?;
    finish(packet_tracer, &id, file).await
}

pub async fn add_building<P: PacketTracer>(
    packet_tracer: &P,
    request: &AddBuildingRequest,
) -> Result<FileEdit, PtError> {
    let name = checked_name(&request.name)?;
    let snapshot = Snapshot::read(packet_tracer).await?;
    let city = snapshot.by_path(&request.inside)?;
    if city.kind != "city" {
        return Err(PtError::InvalidInput(format!(
            "buildings go inside a city, and `{}` is a {}",
            city.name, city.kind
        )));
    }
    let position = match (request.x, request.y) {
        (Some(x), Some(y)) => (x, y),
        (None, None) => DEFAULT_BUILDING_POSITION,
        _ => {
            return Err(PtError::InvalidInput(
                "give both x and y, or neither".into(),
            ));
        }
    };
    let city_id = city.persistent.clone();
    let mut created = String::new();
    let file = edit_saved_network(packet_tracer, |xml| {
        let (edited, id) = pktfile::add_building(xml, &city_id, name, position)
            .map_err(|error| file_error(&error))?;
        created = id;
        Ok(edited)
    })
    .await?;
    finish(packet_tracer, &created, file).await
}

/// Deletes a location and everything inside it. Packet Tracer has no call for this, so
/// the location is cut out of the saved network. Devices must be moved out first; the
/// power units Packet Tracer places in every rack are removed along with it.
pub async fn remove_location<P: PacketTracer>(
    packet_tracer: &P,
    request: &RemoveLocationRequest,
) -> Result<LocationRemoved, PtError> {
    let snapshot = Snapshot::read(packet_tracer).await?;
    let node = snapshot.by_path(&request.path)?;
    if node.parent.is_none() {
        return Err(PtError::InvalidInput(
            "Intercity is the root of the physical workspace and cannot be removed".into(),
        ));
    }
    let inside = format!("{}/", node.path);
    let held: Vec<&str> = snapshot
        .nodes
        .iter()
        .filter(|inner| inner.path.starts_with(&inside))
        .filter_map(|inner| inner.device.as_deref())
        .collect();
    let power_units: Vec<String> = list(packet_tracer)
        .await?
        .devices
        .into_iter()
        .filter(|device| device.model == PDU_MODEL && held.contains(&device.name.as_str()))
        .map(|device| device.name)
        .collect();
    let others: Vec<&str> = held
        .iter()
        .copied()
        .filter(|name| !power_units.iter().any(|unit| unit == name))
        .collect();
    if !others.is_empty() {
        return Err(PtError::InvalidInput(format!(
            "`{}` still holds {}; move them elsewhere with move_to_location or remove them first",
            node.path,
            others.join(", ")
        )));
    }
    let removed = node.path.clone();
    let id = node.persistent.clone();
    for unit in &power_units {
        remove(packet_tracer, unit).await?;
    }
    let file = edit_saved_network(packet_tracer, |xml| {
        pktfile::remove_node(xml, &id).map_err(|error| file_error(&error))
    })
    .await?;
    Ok(LocationRemoved {
        removed,
        power_units_removed: power_units,
        file,
    })
}

async fn finish<P: PacketTracer>(
    packet_tracer: &P,
    persistent: &str,
    file: String,
) -> Result<FileEdit, PtError> {
    let snapshot = Snapshot::read(packet_tracer).await?;
    let node = snapshot.by_persistent(persistent).ok_or_else(|| {
        PtError::UnexpectedReply(format!(
            "the edited location is missing after reopening `{file}`"
        ))
    })?;
    Ok(FileEdit {
        location: snapshot.location_of(node)?,
        file,
    })
}

fn checked_name(name: &str) -> Result<&str, PtError> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') {
        return Err(PtError::InvalidInput(
            "a location name must be non-empty and cannot contain `/`".into(),
        ));
    }
    Ok(name)
}
