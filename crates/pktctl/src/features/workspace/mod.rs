mod files;
mod notes;
mod screenshot;

use rmcp::{
    Json,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_router,
};

pub use files::{
    Cleared, NewRequest, OpenRequest, Opened, SaveRequest, Saved, new_network, open, save,
};
pub use notes::{
    Note, NoteList, NoteRef, NoteRemoved, NoteRequest, NotesRequest, add_note, list_notes,
    remove_note,
};
pub use screenshot::{Screenshot, ScreenshotRequest, View, capture};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = workspace_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "save_network",
        description = "Save the open network as a .pkt file, without any dialog. Give an absolute \
                       path, or omit it to save over the file already open.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn save_network_tool(
        &self,
        Parameters(request): Parameters<SaveRequest>,
    ) -> Result<Json<Saved>, String> {
        save(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "open_network",
        description = "Open a .pkt or .pka file. Unsaved changes in the current network are \
                       discarded unless `save_current_to` gives a path to save them first. \
                       Never shows a dialog.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn open_network_tool(
        &self,
        Parameters(request): Parameters<OpenRequest>,
    ) -> Result<Json<Opened>, String> {
        open(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "new_network",
        description = "Start an empty network. Unsaved changes are discarded unless \
                       `save_current_to` gives a path to save them first.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn new_network_tool(
        &self,
        Parameters(request): Parameters<NewRequest>,
    ) -> Result<Json<Cleared>, String> {
        new_network(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "screenshot",
        description = "Capture Packet Tracer as a PNG image: the logical workspace rendered by \
                       Packet Tracer (default), the physical workspace (`view: physical`, which \
                       switches views for a moment), or the window as it is with any dialog \
                       open (`view: window`). Optionally also write it to an absolute .png path.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn screenshot_tool(
        &self,
        Parameters(request): Parameters<ScreenshotRequest>,
    ) -> Result<CallToolResult, String> {
        let shot = capture(self.packet_tracer(), self.desktop(), &request)
            .await
            .map_err(|error| error.to_string())?;
        let mut content = vec![ContentBlock::image(shot.base64(), "image/png")];
        if let Some(path) = &shot.saved_to {
            content.push(ContentBlock::text(format!("saved to {path}")));
        }
        Ok(CallToolResult::success(content))
    }

    #[tool(
        name = "add_note",
        description = "Write a text note on the logical canvas, for example to label a subnet or \
                       a VLAN. Returns the note with its id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_note_tool(
        &self,
        Parameters(request): Parameters<NoteRequest>,
    ) -> Result<Json<Note>, String> {
        add_note(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "list_notes",
        description = "List the text notes on the logical canvas with their ids and positions. \
                       Port labels drawn at cable ends are left out unless include_port_labels \
                       is true.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_notes_tool(
        &self,
        Parameters(request): Parameters<NotesRequest>,
    ) -> Result<Json<NoteList>, String> {
        list_notes(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "remove_note",
        description = "Delete a note from the logical canvas by id.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn remove_note_tool(
        &self,
        Parameters(request): Parameters<NoteRef>,
    ) -> Result<Json<NoteRemoved>, String> {
        remove_note(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        features::devices::{AddDeviceRequest, add, list},
        packet_tracer::{PtError, scripted::ScriptedPacketTracer},
        testing::{Canvas, FakeDesktop},
    };

    fn canvas() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        (canvas, packet_tracer)
    }

    async fn add_router(packet_tracer: &ScriptedPacketTracer, name: &str) {
        let request = AddDeviceRequest {
            model: "2911".into(),
            name: Some(name.into()),
            ..AddDeviceRequest::default()
        };
        add(packet_tracer, &request).await.unwrap();
    }

    fn save_request(path: Option<&str>) -> SaveRequest {
        SaveRequest {
            path: path.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn saves_opens_and_clears_without_dialogs() {
        let (_canvas, packet_tracer) = canvas();
        add_router(&packet_tracer, "R1").await;
        let saved = save(&packet_tracer, &save_request(Some("/labs/gamc.pkt")))
            .await
            .unwrap();
        assert_eq!(saved.path, "/labs/gamc.pkt");
        assert!(saved.bytes > 0);

        add_router(&packet_tracer, "R2").await;
        save(&packet_tracer, &save_request(None)).await.unwrap();

        let cleared = new_network(&packet_tracer, &NewRequest::default())
            .await
            .unwrap();
        assert!(cleared.cleared);
        assert!(list(&packet_tracer).await.unwrap().devices.is_empty());

        let opened = open(
            &packet_tracer,
            &OpenRequest {
                path: "/labs/gamc.pkt".into(),
                save_current_to: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(opened.devices, 2);
    }

    #[tokio::test]
    async fn keeps_current_work_when_asked_before_opening() {
        let (_canvas, packet_tracer) = canvas();
        add_router(&packet_tracer, "R1").await;
        save(&packet_tracer, &save_request(Some("/labs/a.pkt")))
            .await
            .unwrap();
        new_network(&packet_tracer, &NewRequest::default())
            .await
            .unwrap();
        add_router(&packet_tracer, "DRAFT").await;

        let opened = open(
            &packet_tracer,
            &OpenRequest {
                path: "/labs/a.pkt".into(),
                save_current_to: Some("/labs/draft.pkt".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(opened.saved_previous.unwrap().path, "/labs/draft.pkt");
        let draft = open(
            &packet_tracer,
            &OpenRequest {
                path: "/labs/draft.pkt".into(),
                save_current_to: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(draft.devices, 1);
    }

    #[tokio::test]
    async fn explains_file_problems() {
        let (_canvas, packet_tracer) = canvas();
        let never_saved = save(&packet_tracer, &save_request(None)).await.unwrap_err();
        assert!(never_saved.to_string().contains("never been saved"));
        let missing = open(
            &packet_tracer,
            &OpenRequest {
                path: "/labs/missing.pkt".into(),
                save_current_to: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            missing,
            PtError::NotFound("file `/labs/missing.pkt`".into())
        );
        assert!(
            save(&packet_tracer, &save_request(Some("relative.pkt")))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn notes_round_trip() {
        let (_canvas, packet_tracer) = canvas();
        let note = add_note(
            &packet_tracer,
            &NoteRequest {
                x: 120,
                y: 40,
                text: "LAN 192.168.10.0/24".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (note.x, note.y, note.text.as_str()),
            (120, 40, "LAN 192.168.10.0/24")
        );
        assert_eq!(
            list_notes(&packet_tracer, &NotesRequest::default())
                .await
                .unwrap()
                .notes,
            vec![note.clone()]
        );

        remove_note(
            &packet_tracer,
            &NoteRef {
                id: note.id.clone(),
            },
        )
        .await
        .unwrap();
        assert!(
            list_notes(&packet_tracer, &NotesRequest::default())
                .await
                .unwrap()
                .notes
                .is_empty()
        );
        assert!(matches!(
            remove_note(&packet_tracer, &NoteRef { id: note.id }).await,
            Err(PtError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn port_labels_are_hidden_unless_requested() {
        let (_canvas, packet_tracer) = canvas();
        add_router(&packet_tracer, "R1").await;
        add_router(&packet_tracer, "R2").await;
        crate::features::links::connect(
            &packet_tracer,
            &crate::features::links::ConnectRequest {
                device_a: "R1".into(),
                port_a: "GigabitEthernet0/0".into(),
                device_b: "R2".into(),
                port_b: "GigabitEthernet0/1".into(),
                cable: crate::features::links::Cable::Auto,
            },
        )
        .await
        .unwrap();
        add_note(
            &packet_tracer,
            &NoteRequest {
                x: 10,
                y: 10,
                text: "WAN".into(),
            },
        )
        .await
        .unwrap();

        let visible = list_notes(&packet_tracer, &NotesRequest::default())
            .await
            .unwrap();
        let texts: Vec<_> = visible
            .notes
            .iter()
            .map(|note| note.text.as_str())
            .collect();
        assert_eq!(texts, ["WAN"]);

        let everything = list_notes(
            &packet_tracer,
            &NotesRequest {
                include_port_labels: true,
            },
        )
        .await
        .unwrap();
        let texts: Vec<_> = everything
            .notes
            .iter()
            .map(|note| note.text.as_str())
            .collect();
        assert_eq!(texts, ["WAN", "Gig0/0", "Gig0/1"]);
    }

    #[tokio::test(start_paused = true)]
    async fn physical_captures_switch_views_and_switch_back() {
        let (canvas, packet_tracer) = canvas();
        let desktop = FakeDesktop::default();
        let shot = capture(
            &packet_tracer,
            &desktop,
            &ScreenshotRequest {
                view: View::Physical,
                ..ScreenshotRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(shot.png, FakeDesktop::PNG);
        assert_eq!(desktop.captures(), 1);
        assert!(!canvas.is_physical_mode(), "the logical view is restored");

        capture(
            &packet_tracer,
            &desktop,
            &ScreenshotRequest {
                view: View::Window,
                ..ScreenshotRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(desktop.captures(), 2);
    }

    #[tokio::test]
    async fn screenshots_are_pngs_and_can_be_written() {
        let (_canvas, packet_tracer) = canvas();
        let shot = capture(
            &packet_tracer,
            &FakeDesktop::default(),
            &ScreenshotRequest::default(),
        )
        .await
        .unwrap();
        assert!(shot.png.starts_with(b"\x89PNG"));
        assert!(!shot.base64().is_empty());

        let path = std::env::temp_dir().join(format!("pktctl-shot-{}.png", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        let saved = capture(
            &packet_tracer,
            &FakeDesktop::default(),
            &ScreenshotRequest {
                save_to: Some(path.clone()),
                ..ScreenshotRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), saved.png);
        std::fs::remove_file(path).unwrap();
    }
}
