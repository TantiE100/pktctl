use std::io::Cursor;

use xcap::{
    Window,
    image::{ImageFormat, RgbaImage, imageops},
};

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
        let windows: Vec<Window> = Window::all()
            .map_err(|error| capture_error(&error))?
            .into_iter()
            .filter(|window| {
                window.app_name().is_ok_and(|app| app.contains(APP_NAME))
                    && !window.is_minimized().unwrap_or(true)
            })
            .collect();
        let main = windows
            .iter()
            .max_by_key(|window| area(window))
            .ok_or_else(|| {
                PtError::Unreachable("no visible Packet Tracer window to capture".into())
            })?;
        let mut image = main
            .capture_image()
            .map_err(|error| capture_error(&error))?;
        overlay_dialogs(&mut image, main, &windows);
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| {
                PtError::Transport(format!("could not encode the capture: {error}"))
            })?;
        Ok(png.into_inner())
    }
}

fn area(window: &Window) -> u64 {
    u64::from(window.width().unwrap_or(0)) * u64::from(window.height().unwrap_or(0))
}

/// Dialogs are separate windows; paints the ones in front of the main window onto
/// it, back to front, so the capture shows what the user sees.
fn overlay_dialogs(image: &mut RgbaImage, main: &Window, windows: &[Window]) {
    let (Ok(main_id), Ok(main_x), Ok(main_y), Ok(main_z), Ok(main_width)) =
        (main.id(), main.x(), main.y(), main.z(), main.width())
    else {
        return;
    };
    let scale = i64::from((image.width() / main_width.max(1)).max(1));
    let mut dialogs: Vec<(i32, &Window)> = windows
        .iter()
        .filter(|window| window.id().is_ok_and(|id| id != main_id))
        .filter_map(|window| window.z().ok().map(|z| (z, window)))
        .filter(|(z, _)| *z > main_z)
        .collect();
    dialogs.sort_by_key(|(z, _)| *z);
    for (_, dialog) in dialogs {
        let (Ok(x), Ok(y), Ok(top)) = (dialog.x(), dialog.y(), dialog.capture_image()) else {
            continue;
        };
        let left = i64::from(x - main_x) * scale;
        let upper = i64::from(y - main_y) * scale;
        imageops::overlay(image, &top, left, upper);
    }
}

fn capture_error(error: &xcap::XCapError) -> PtError {
    PtError::Rejected(format!(
        "could not capture Packet Tracer's window ({error}); on macOS allow Screen Recording for \
         the app running pktctl in System Settings > Privacy & Security"
    ))
}
