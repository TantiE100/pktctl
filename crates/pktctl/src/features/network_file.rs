use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    features::{
        devices::{list, remove},
        workspace::{OpenRequest, SaveRequest, open, save},
    },
    packet_tracer::{PacketTracer, PtError},
};

const PDU_MODEL: &str = "Power Distribution Device";
static SCRATCH_FILES: AtomicUsize = AtomicUsize::new(0);

/// Saves the network, applies `edit` to the saved XML, reopens it and removes the
/// power units Packet Tracer adds on open. Returns the file used.
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

pub(crate) fn file_error(error: &pktfile::PktError) -> PtError {
    PtError::Rejected(format!("could not edit the network file: {error}"))
}

pub(crate) fn local_file(path: &str, error: &std::io::Error) -> PtError {
    PtError::Transport(format!(
        "could not access `{path}` ({error}); editing the network file needs pktctl to run on \
         the same computer as Packet Tracer"
    ))
}
