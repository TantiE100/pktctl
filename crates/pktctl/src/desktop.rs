use std::io::Cursor;

use xcap::{Window, image::ImageFormat};

use crate::packet_tracer::PtError;

const APP_NAME: &str = "Packet Tracer";

/// Captures Packet Tracer's own window from the operating system, for views the
/// IPC API cannot render. Only that window is ever captured, never the screen.
pub trait Desktop: Send + Sync + 'static {
    fn capture_packet_tracer(&self) -> Result<Vec<u8>, PtError>;
}

#[derive(Debug, Default)]
pub struct SystemDesktop;

impl Desktop for SystemDesktop {
    fn capture_packet_tracer(&self) -> Result<Vec<u8>, PtError> {
        let windows = Window::all().map_err(|error| capture_error(&error))?;
        let main = windows
            .into_iter()
            .filter(|window| {
                window.app_name().is_ok_and(|app| app.contains(APP_NAME))
                    && !window.is_minimized().unwrap_or(true)
            })
            .max_by_key(|window| {
                u64::from(window.width().unwrap_or(0)) * u64::from(window.height().unwrap_or(0))
            })
            .ok_or_else(|| {
                PtError::Unreachable("no visible Packet Tracer window to capture".into())
            })?;
        let image = main
            .capture_image()
            .map_err(|error| capture_error(&error))?;
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| {
                PtError::Transport(format!("could not encode the capture: {error}"))
            })?;
        Ok(png.into_inner())
    }
}

fn capture_error(error: &xcap::XCapError) -> PtError {
    PtError::Rejected(format!(
        "could not capture Packet Tracer's window ({error}); on macOS allow Screen Recording for \
         the app running pktctl in System Settings > Privacy & Security"
    ))
}
