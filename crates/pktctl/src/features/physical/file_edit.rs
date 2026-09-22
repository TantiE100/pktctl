use std::sync::atomic::{AtomicUsize, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::tree::{Location, Snapshot};
use crate::{
    features::{
        devices::{list, remove},
        workspace::{OpenRequest, SaveRequest, open, save},
    },
    packet_tracer::{PacketTracer, PtError},
};

const PDU_MODEL: &str = "Power Distribution Device";
static SCRATCH_FILES: AtomicUsize = AtomicUsize::new(0);
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileEdit {
    pub location: Location,
    /// The file the network was saved to, edited and reopened from.
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

async fn edit_saved_network<P, F>(packet_tracer: &P, edit: F) -> Result<String, PtError>
where
    P: PacketTracer,
    F: FnOnce(&str) -> Result<String, PtError>,
{
    let before: Vec<String> = list(packet_tracer)
        .await?
        .devices
        .into_iter()
        .map(|device| device.name)
        .collect();
    let saved = match save(packet_tracer, &SaveRequest::default()).await {
        Ok(saved) => saved,
        Err(PtError::InvalidInput(_)) => {
            let scratch = std::env::temp_dir().join(format!(
                "pktctl-network-{}-{}.pkt",
                std::process::id(),
                SCRATCH_FILES.fetch_add(1, Ordering::Relaxed)
            ));
            save(
                packet_tracer,
                &SaveRequest {
                    path: Some(scratch.display().to_string()),
                },
            )
            .await?
        }
        Err(error) => return Err(error),
    };
    let path = saved.path;

    let bytes = std::fs::read(&path).map_err(|error| local_file(&path, &error))?;
    let xml = pktfile::decode(&bytes).map_err(|error| file_error(&error))?;
    let edited = edit(&xml)?;
    let bytes = pktfile::encode(&edited).map_err(|error| file_error(&error))?;
    std::fs::write(&path, bytes).map_err(|error| local_file(&path, &error))?;

    open(
        packet_tracer,
        &OpenRequest {
            path: path.clone(),
            save_current_to: None,
        },
    )
    .await?;
    for device in list(packet_tracer).await?.devices {
        if device.model == PDU_MODEL && !before.contains(&device.name) {
            remove(packet_tracer, &device.name).await?;
        }
    }
    Ok(path)
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

fn file_error(error: &pktfile::PktError) -> PtError {
    PtError::Rejected(format!("could not edit the network file: {error}"))
}

fn local_file(path: &str, error: &std::io::Error) -> PtError {
    PtError::Transport(format!(
        "could not access `{path}` ({error}); editing the network file needs pktctl to run on \
         the same computer as Packet Tracer"
    ))
}
