use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    features::{
        devices::{list, remove},
        paths::app_window,
        power::fast_forward,
        workspace::{OpenRequest, open},
    },
    packet_tracer::{PacketTracer, PtError},
};

pub(crate) const PDU_MODEL: &str = "Power Distribution Device";
static SCRATCH_FILES: AtomicUsize = AtomicUsize::new(0);

/// Takes the open network as `.pkt` bytes straight from Packet Tracer, applies `edit`
/// to its XML, writes the result to a new temporary file and opens that, removing the
/// power units Packet Tracer adds on open, and fast forwards time so spanning tree and
/// DHCP settle again after the reload. The user's own file is never written.
pub(crate) async fn edit_saved_network<P, F>(packet_tracer: &P, edit: F) -> Result<String, PtError>
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
    let current = packet_tracer
        .call(app_window().method("fileSaveToBytes", []))
        .await?
        .into_bytes()
        .ok_or_else(|| PtError::UnexpectedReply("fileSaveToBytes should return bytes".into()))?;
    let xml = pktfile::decode(&current).map_err(|error| file_error(&error))?;
    let edited = pktfile::encode(&edit(&xml)?).map_err(|error| file_error(&error))?;

    let scratch = std::env::temp_dir().join(format!(
        "pktctl-edit-{}-{}.pkt",
        std::process::id(),
        SCRATCH_FILES.fetch_add(1, Ordering::Relaxed)
    ));
    let path = scratch.display().to_string();
    tokio::fs::write(&scratch, edited)
        .await
        .map_err(|error| local_file(&path, &error))?;

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
    fast_forward(packet_tracer).await?;
    Ok(path)
}

pub(crate) fn file_error(error: &pktfile::PktError) -> PtError {
    PtError::Rejected(format!("could not edit the network file: {error}"))
}

pub(crate) fn local_file(path: &str, error: &std::io::Error) -> PtError {
    PtError::Transport(format!(
        "could not access `{path}` ({error}); editing the network file needs pktctl to run on \
         the same computer as Packet Tracer"
    ))
}
