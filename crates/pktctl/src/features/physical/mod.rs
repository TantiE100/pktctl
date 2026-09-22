mod arrange;
mod background;
mod file_edit;
mod place;
mod tree;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use arrange::{ArrangeRequest, Arrangement, Placed, arrange_devices};
pub use background::{Background, BackgroundRequest, set_background};
pub use file_edit::{
    FileEdit, LocationRemoved, RemoveLocationRequest, RenameLocationRequest, remove_location,
    rename_location,
};
pub(crate) use place::SCENE;
pub use place::{
    AddLocationRequest, MoveRequest, Moved, NewLocation, add_location, move_to_location,
};
pub(crate) use tree::Snapshot;
pub use tree::{Location, LocationList};

use crate::{
    features::paths::app_window,
    packet_tracer::{PacketTracer, PtError, expect_bool},
    server::PktctlServer,
};

pub async fn list_locations<P: PacketTracer>(packet_tracer: &P) -> Result<LocationList, PtError> {
    Ok(tree::Snapshot::read(packet_tracer).await?.locations())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Logical,
    Physical,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ViewRequest {
    /// Which workspace Packet Tracer should show.
    pub view: View,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Shown {
    pub physical: bool,
}

pub async fn show_workspace<P: PacketTracer>(
    packet_tracer: &P,
    request: &ViewRequest,
) -> Result<Shown, PtError> {
    let switch = app_window().method("getPLSwitch", []);
    let method = match request.view {
        View::Logical => "showLogicalMode",
        View::Physical => "showPhysicalMode",
    };
    packet_tracer.call(switch.method(method, [])).await?;
    let physical = packet_tracer
        .call(app_window().method("isPhysicalMode", []))
        .await?;
    Ok(Shown {
        physical: expect_bool(&physical, "isPhysicalMode")?,
    })
}

#[tool_router(router = physical_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_locations",
        description = "List the physical workspace: Intercity, cities, buildings, wiring closets \
                       and racks, each with its path, position and the devices placed in it.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_locations_tool(&self) -> Result<Json<LocationList>, String> {
        list_locations(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_location",
        description = "Create a place in the physical workspace: a `city`, a `building`, a \
                       `wiring_closet`, or furniture to arrange devices on (`rack`, `table`, \
                       `shelf`, `cable_pegboard`, `container`), with an optional name and \
                       position. Cities and closets use Packet Tracer's own buttons; the rest \
                       are written into the network file and come back with the temporary copy \
                       Packet Tracer now has open, so save with save_network to keep them.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_location_tool(
        &self,
        Parameters(request): Parameters<AddLocationRequest>,
    ) -> Result<Json<FileEdit>, String> {
        add_location(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "move_to_location",
        description = "Move a device, or a whole location such as a wiring closet, to another \
                       place in the physical workspace, for example a switch into \
                       `Home City/Corporate Office/Main Wiring Closet`. Devices moved into a \
                       wiring closet land on its rack or table.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn move_to_location_tool(
        &self,
        Parameters(request): Parameters<MoveRequest>,
    ) -> Result<Json<Moved>, String> {
        move_to_location(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "rename_location",
        description = "Rename a city, building, wiring closet or other location. Packet Tracer \
                       has no call for this, so pktctl takes the network as bytes, edits them and \
                       opens the result as a temporary copy; your own file is not written, save \
                       with save_network and a path to keep the change.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn rename_location_tool(
        &self,
        Parameters(request): Parameters<RenameLocationRequest>,
    ) -> Result<Json<FileEdit>, String> {
        rename_location(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "remove_location",
        description = "Delete a city, building, wiring closet or rack with everything inside \
                       it. Devices must be moved out first (move_to_location); the power units \
                       Packet Tracer puts in racks go with it. Packet Tracer has no call for \
                       this, so pktctl edits the network as bytes and opens the result as a \
                       temporary copy; save with save_network and a path to keep the change.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn remove_location_tool(
        &self,
        Parameters(request): Parameters<RemoveLocationRequest>,
    ) -> Result<Json<LocationRemoved>, String> {
        remove_location(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "arrange_devices",
        description = "Lay devices out inside a room, building, rack or table of the physical \
                       workspace, moving in the ones that are somewhere else. Positions are \
                       percentages of the room, which is how Packet Tracer draws them: leave \
                       them out for an even grid, set `columns` and `margin_percent` to shape \
                       it, or give exact `spots` like [[20, 30], [60, 30]].",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn arrange_devices_tool(
        &self,
        Parameters(request): Parameters<ArrangeRequest>,
    ) -> Result<Json<Arrangement>, String> {
        arrange_devices(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "set_background",
        description = "Paper a physical location (a city, building, room or rack) with a \
                       background image, or the logical workspace when no location is given. \
                       Takes one of Packet Tracer's own backgrounds (grid_10x10, grid_25x25, \
                       grid_50x50, grid_100x100, city, building, intercity, container) or the \
                       absolute path of an image; an empty image clears it.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn set_background_tool(
        &self,
        Parameters(request): Parameters<BackgroundRequest>,
    ) -> Result<Json<Background>, String> {
        set_background(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "show_workspace",
        description = "Switch Packet Tracer's main window between the logical and the physical \
                       workspace, for example before a screenshot or a class demo.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn show_workspace_tool(
        &self,
        Parameters(request): Parameters<ViewRequest>,
    ) -> Result<Json<Shown>, String> {
        show_workspace(self.packet_tracer(), &request)
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
        features::devices::{AddDeviceRequest, add},
        packet_tracer::PtError,
        packet_tracer::scripted::ScriptedPacketTracer,
        testing::Canvas,
    };

    const OFFICE: &str = "Home City/Corporate Office";
    const MAIN_CLOSET: &str = "Home City/Corporate Office/Main Wiring Closet";

    async fn lab() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        for (model, name) in [("2911", "R1"), ("2960-24TT", "S1"), ("PC-PT", "PC1")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        (canvas, packet_tracer)
    }

    fn move_device(name: &str, into: &str) -> MoveRequest {
        MoveRequest {
            device: Some(name.into()),
            into: into.into(),
            ..MoveRequest::default()
        }
    }

    #[tokio::test]
    async fn lists_locations_with_devices_by_their_real_names() {
        let (_canvas, packet_tracer) = lab().await;
        let list = list_locations(&packet_tracer).await.unwrap();
        let paths: Vec<&str> = list.locations.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "",
                "Home City",
                OFFICE,
                MAIN_CLOSET,
                "Home City/Corporate Office/Main Wiring Closet/Rack"
            ]
        );
        let rack = list.locations.last().unwrap();
        assert_eq!(rack.kind, "rack");
        assert_eq!(rack.devices, ["R1", "S1"]);
        assert_eq!(list.locations[2].devices, ["PC1"]);
        assert_eq!(list.locations[3].kind, "wiring_closet");
    }

    #[tokio::test]
    async fn creates_closets_where_asked_and_cities_in_intercity() {
        let (_canvas, packet_tracer) = lab().await;
        let city = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::City,
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (city.location.path.as_str(), city.location.kind.as_str()),
            ("City", "city")
        );

        let closet = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some(format!("Intercity/{OFFICE}")),
                x_percent: Some(30.0),
                y_percent: Some(12.0),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap()
        .location;
        assert_eq!(closet.path, "Home City/Corporate Office/Wiring Closet");
        assert_eq!(
            (closet.x, closet.y),
            (1033, 259),
            "percentages of the room become the coordinates Packet Tracer keeps"
        );

        let refused = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::City,
                inside: Some("Home City".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            refused.to_string().contains("cannot go inside"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn moves_devices_across_the_tree_even_when_their_uuid_changes() {
        let (canvas, packet_tracer) = lab().await;
        add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::City,
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();
        add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some("City".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();

        let moved = move_to_location(&packet_tracer, &move_device("S1", "City/Wiring Closet"))
            .await
            .unwrap();
        assert_eq!(moved.now_in, "City/Wiring Closet/Rack");
        assert_eq!(canvas.physical_parent("S1").as_deref(), Some("Rack"));

        let back = move_to_location(&packet_tracer, &move_device("S1", MAIN_CLOSET))
            .await
            .unwrap();
        assert_eq!(back.now_in, format!("{MAIN_CLOSET}/Rack"));

        let pc = move_to_location(&packet_tracer, &move_device("PC1", "City"))
            .await
            .unwrap();
        assert_eq!(pc.now_in, "City");
    }

    #[tokio::test]
    async fn moves_whole_locations_and_guards_against_loops() {
        let (_canvas, packet_tracer) = lab().await;
        add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some(OFFICE.into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();
        let moved = move_to_location(
            &packet_tracer,
            &MoveRequest {
                location: Some(format!("{OFFICE}/Wiring Closet")),
                into: "Home City".into(),
                ..MoveRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(moved.now_in, "Home City");

        let into_itself = move_to_location(
            &packet_tracer,
            &MoveRequest {
                location: Some("Home City".into()),
                into: OFFICE.into(),
                ..MoveRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(into_itself.to_string().contains("inside itself"));

        let both = move_to_location(
            &packet_tracer,
            &MoveRequest {
                device: Some("R1".into()),
                location: Some("Home City".into()),
                into: String::new(),
                ..MoveRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(both.to_string().contains("exactly one"));
    }

    #[tokio::test]
    async fn refuses_ambiguous_duplicate_targets() {
        let (_canvas, packet_tracer) = lab().await;
        for _ in 0..2 {
            add_location(
                &packet_tracer,
                &AddLocationRequest {
                    kind: NewLocation::City,
                    ..AddLocationRequest::default()
                },
            )
            .await
            .unwrap();
        }
        let paths: Vec<String> = list_locations(&packet_tracer)
            .await
            .unwrap()
            .locations
            .into_iter()
            .map(|location| location.path)
            .collect();
        assert!(paths.contains(&"City#2".to_owned()));
        let error = move_to_location(&packet_tracer, &move_device("PC1", "City#2"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("shares its name"), "{error}");

        let count = paths.len();
        let refused = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some("City#2".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(refused.to_string().contains("shares its name"), "{refused}");
        let after = list_locations(&packet_tracer)
            .await
            .unwrap()
            .locations
            .len();
        assert_eq!(
            after, count,
            "nothing is created when the target is unreachable"
        );
    }

    #[tokio::test]
    async fn renames_locations_through_the_saved_file() {
        let (_canvas, packet_tracer) = lab().await;
        let renamed = rename_location(
            &packet_tracer,
            &RenameLocationRequest {
                path: "Home City".into(),
                name: "Cochabamba".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.location.path, "Cochabamba");
        assert!(std::path::Path::new(renamed.file.as_deref().unwrap()).exists());
        let list = list_locations(&packet_tracer).await.unwrap();
        assert!(list.locations.iter().any(|location| location.path
            == "Cochabamba/Corporate Office/Main Wiring Closet/Rack"
            && location.devices == ["R1", "S1"]));
        std::fs::remove_file(renamed.file.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn file_edits_never_write_the_users_own_file() {
        let (_canvas, packet_tracer) = lab().await;
        let own = std::env::temp_dir().join(format!("pktctl-own-{}.pkt", std::process::id()));
        let own_path = own.display().to_string();
        crate::features::workspace::save(
            &packet_tracer,
            &crate::features::workspace::SaveRequest {
                path: Some(own_path.clone()),
            },
        )
        .await
        .unwrap();
        let original = std::fs::read(&own).unwrap();

        let renamed = rename_location(
            &packet_tracer,
            &RenameLocationRequest {
                path: "Home City".into(),
                name: "Cochabamba".into(),
            },
        )
        .await
        .unwrap();
        assert_ne!(renamed.file.as_deref(), Some(own_path.as_str()));
        assert_eq!(std::fs::read(&own).unwrap(), original);
        std::fs::remove_file(own).unwrap();
        std::fs::remove_file(renamed.file.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn duplicates_can_be_renamed_apart_and_then_targeted() {
        let (_canvas, packet_tracer) = lab().await;
        for _ in 0..2 {
            add_location(
                &packet_tracer,
                &AddLocationRequest {
                    kind: NewLocation::City,
                    ..AddLocationRequest::default()
                },
            )
            .await
            .unwrap();
        }
        let renamed = rename_location(
            &packet_tracer,
            &RenameLocationRequest {
                path: "City#2".into(),
                name: "El Alto".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.location.path, "El Alto");
        let moved = move_to_location(&packet_tracer, &move_device("PC1", "El Alto"))
            .await
            .unwrap();
        assert_eq!(moved.now_in, "El Alto");
        std::fs::remove_file(renamed.file.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn removes_only_locations_without_devices() {
        let (_canvas, packet_tracer) = lab().await;
        let remove = |path: &str| RemoveLocationRequest { path: path.into() };
        let root = remove_location(&packet_tracer, &remove(""))
            .await
            .unwrap_err();
        assert!(root.to_string().contains("Intercity"), "{root}");

        let closet = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some("Home City".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap()
        .location;
        move_to_location(&packet_tracer, &move_device("PC1", &closet.path))
            .await
            .unwrap();
        let busy = remove_location(&packet_tracer, &remove(&closet.path))
            .await
            .unwrap_err();
        assert!(busy.to_string().contains("PC1"), "{busy}");

        move_to_location(&packet_tracer, &move_device("PC1", OFFICE))
            .await
            .unwrap();
        let removed = remove_location(&packet_tracer, &remove(&closet.path))
            .await
            .unwrap();
        assert_eq!(removed.removed, closet.path);
        let paths: Vec<String> = list_locations(&packet_tracer)
            .await
            .unwrap()
            .locations
            .into_iter()
            .map(|location| location.path)
            .collect();
        assert!(!paths.contains(&closet.path), "{paths:?}");
        assert!(paths.iter().any(|path| path == MAIN_CLOSET));
        std::fs::remove_file(removed.file).unwrap();
    }

    #[tokio::test]
    async fn adds_furniture_and_names_it() {
        let (_canvas, packet_tracer) = lab().await;
        let table = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::Table,
                inside: Some(MAIN_CLOSET.into()),
                name: Some("Mesa de trabajo".into()),
                x_percent: Some(30.0),
                y_percent: Some(40.0),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            table.location.path,
            format!("{MAIN_CLOSET}/Mesa de trabajo")
        );
        assert_eq!(table.location.kind, "stackable_table");
        assert!(table.file.is_some(), "furniture goes through the file");
        std::fs::remove_file(table.file.unwrap()).unwrap();

        let building = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::Building,
                inside: Some("Home City".into()),
                name: Some("Alcaldía GAMC".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(building.location.kind, "building");
        std::fs::remove_file(building.file.unwrap()).unwrap();

        let closet = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some("Home City".into()),
                name: Some("Cuarto de Equipos".into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(closet.location.path, "Home City/Cuarto de Equipos");
        assert!(closet.file.is_none(), "closets use Packet Tracer's button");

        let refused = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::Building,
                inside: Some(MAIN_CLOSET.into()),
                ..AddLocationRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            refused.to_string().contains("cannot go inside"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn arranges_devices_in_rows() {
        let (_canvas, packet_tracer) = lab().await;
        let arranged = arrange_devices(
            &packet_tracer,
            &ArrangeRequest {
                location: OFFICE.into(),
                devices: vec!["PC1".into(), "R1".into(), "S1".into()],
                columns: Some(2),
                margin_percent: Some(10.0),
                ..ArrangeRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            arranged
                .devices
                .iter()
                .map(|placed| (placed.device.as_str(), placed.x_percent, placed.y_percent))
                .collect::<Vec<_>>(),
            [("PC1", 10.0, 10.0), ("R1", 90.0, 10.0), ("S1", 10.0, 90.0)]
        );
        let list = list_locations(&packet_tracer).await.unwrap();
        let office = list
            .locations
            .iter()
            .find(|location| location.path == OFFICE)
            .unwrap();
        assert!(office.devices.contains(&"R1".to_owned()), "{office:?}");

        let everything = arrange_devices(
            &packet_tracer,
            &ArrangeRequest {
                location: OFFICE.into(),
                ..ArrangeRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(everything.devices.len(), 3);
        let bad = arrange_devices(
            &packet_tracer,
            &ArrangeRequest {
                location: OFFICE.into(),
                columns: Some(0),
                ..ArrangeRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad, PtError::InvalidInput(_)), "{bad}");
    }

    #[tokio::test]
    async fn papers_locations_and_the_logical_workspace() {
        let (_canvas, packet_tracer) = lab().await;
        let room = set_background(
            &packet_tracer,
            &BackgroundRequest {
                location: Some(MAIN_CLOSET.into()),
                image: "grid_25x25".into(),
                tiled: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(room.image, "../art/Background/grid_25x25.png");
        assert_eq!(room.target, MAIN_CLOSET);

        let logical = set_background(
            &packet_tracer,
            &BackgroundRequest {
                image: "/tmp/plano.png".into(),
                ..BackgroundRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(logical.target, "logical workspace");

        let unknown = set_background(
            &packet_tracer,
            &BackgroundRequest {
                image: "plano.png".into(),
                ..BackgroundRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(unknown, PtError::InvalidInput(_)), "{unknown}");
    }

    #[tokio::test]
    async fn switches_between_workspaces() {
        let (_canvas, packet_tracer) = lab().await;
        let shown = show_workspace(
            &packet_tracer,
            &ViewRequest {
                view: View::Physical,
            },
        )
        .await
        .unwrap();
        assert!(shown.physical);
        let shown = show_workspace(&packet_tracer, &ViewRequest::default())
            .await
            .unwrap();
        assert!(!shown.physical);
    }
}
