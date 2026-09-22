mod file_edit;
mod place;
mod tree;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use file_edit::{
    AddBuildingRequest, FileEdit, RenameLocationRequest, add_building, rename_location,
};
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
        description = "Create a city in Intercity, or a wiring closet in Intercity, a city or a \
                       building. Returns the new location with its path.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_location_tool(
        &self,
        Parameters(request): Parameters<AddLocationRequest>,
    ) -> Result<Json<Location>, String> {
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
                       wiring closet are mounted in its rack.",
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
                       has no call for this, so pktctl saves the network, edits the saved file \
                       and reopens it; the network ends up saved (to its current file, or to a \
                       temporary file if it was never saved).",
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
        name = "add_building",
        description = "Create a named building inside a city. Packet Tracer has no call for \
                       this, so pktctl saves the network, adds the building to the saved file \
                       and reopens it; the network ends up saved (to its current file, or to a \
                       temporary file if it was never saved).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_building_tool(
        &self,
        Parameters(request): Parameters<AddBuildingRequest>,
    ) -> Result<Json<FileEdit>, String> {
        add_building(self.packet_tracer(), &request)
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
        assert_eq!((city.path.as_str(), city.kind.as_str()), ("City", "city"));

        let closet = add_location(
            &packet_tracer,
            &AddLocationRequest {
                kind: NewLocation::WiringCloset,
                inside: Some(format!("Intercity/{OFFICE}")),
                x: Some(300),
                y: Some(120),
            },
        )
        .await
        .unwrap();
        assert_eq!(closet.path, "Home City/Corporate Office/Wiring Closet");
        assert_eq!((closet.x, closet.y), (300, 120));

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
        assert!(std::path::Path::new(&renamed.file).exists());
        let list = list_locations(&packet_tracer).await.unwrap();
        assert!(list.locations.iter().any(|location| location.path
            == "Cochabamba/Corporate Office/Main Wiring Closet/Rack"
            && location.devices == ["R1", "S1"]));
        std::fs::remove_file(renamed.file).unwrap();
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
        std::fs::remove_file(renamed.file).unwrap();
    }

    #[tokio::test]
    async fn adds_named_buildings_to_cities_only() {
        let (_canvas, packet_tracer) = lab().await;
        let building = add_building(
            &packet_tracer,
            &AddBuildingRequest {
                inside: "Home City".into(),
                name: "Alcaldía GAMC".into(),
                x: Some(400),
                y: Some(150),
            },
        )
        .await
        .unwrap();
        assert_eq!(building.location.path, "Home City/Alcaldía GAMC");
        assert_eq!(building.location.kind, "building");
        assert_eq!((building.location.x, building.location.y), (400, 150));
        std::fs::remove_file(&building.file).unwrap();

        let refused = add_building(
            &packet_tracer,
            &AddBuildingRequest {
                inside: "Home City/Corporate Office".into(),
                name: "Anexo".into(),
                ..AddBuildingRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(refused.to_string().contains("inside a city"), "{refused}");

        let bad_name = rename_location(
            &packet_tracer,
            &RenameLocationRequest {
                path: "Home City".into(),
                name: "a/b".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad_name, PtError::InvalidInput(_)));
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
