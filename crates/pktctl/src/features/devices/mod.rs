mod list;
mod manage;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};

pub use list::{Device, DeviceList, count, describe, list};
pub use manage::{
    AddDeviceRequest, DeviceRef, MoveRequest, Removed, RenameRequest, add, relocate, remove, rename,
};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = devices_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_devices",
        description = "List every device in the open network with its model, kind (router, \
                       switch, pc, ...) and canvas position. Use the names to target devices \
                       in other tools.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_devices_tool(&self) -> Result<Json<DeviceList>, String> {
        list(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_device",
        description = "Add a device to the logical workspace by model name (see list_models), \
                       optionally naming it and placing it at x/y. Routers and switches boot \
                       immediately. Returns the device as Packet Tracer now shows it.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_device_tool(
        &self,
        Parameters(request): Parameters<AddDeviceRequest>,
    ) -> Result<Json<Device>, String> {
        add(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "remove_device",
        description = "Delete a device and all of its links from the network.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn remove_device_tool(
        &self,
        Parameters(request): Parameters<DeviceRef>,
    ) -> Result<Json<Removed>, String> {
        remove(self.packet_tracer(), &request.name)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "rename_device",
        description = "Rename a device. The new name must not be taken by another device.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn rename_device_tool(
        &self,
        Parameters(request): Parameters<RenameRequest>,
    ) -> Result<Json<Device>, String> {
        rename(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "move_device",
        description = "Move a device to a new position on the logical canvas.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn move_device_tool(
        &self,
        Parameters(request): Parameters<MoveRequest>,
    ) -> Result<Json<Device>, String> {
        relocate(self.packet_tracer(), &request)
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
        packet_tracer::{PtError, scripted::ScriptedPacketTracer},
        testing::Canvas,
    };

    fn canvas() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        (canvas, packet_tracer)
    }

    fn add_request(model: &str, name: Option<&str>) -> AddDeviceRequest {
        AddDeviceRequest {
            model: model.into(),
            name: name.map(str::to_owned),
            x: None,
            y: None,
        }
    }

    #[tokio::test]
    async fn adds_devices_with_packet_tracer_names_on_a_grid() {
        let (canvas, packet_tracer) = canvas();
        let router = add(&packet_tracer, &add_request("2911", None))
            .await
            .unwrap();
        let pc = add(&packet_tracer, &add_request("pc-pt", None))
            .await
            .unwrap();

        assert_eq!(
            (
                router.name.as_str(),
                router.kind.as_str(),
                router.x,
                router.y
            ),
            ("Router0", "router", 100.0, 100.0)
        );
        assert_eq!(
            (pc.name.as_str(), pc.model.as_str(), pc.x),
            ("PC0", "PC-PT", 220.0)
        );
        assert_eq!(canvas.device_names(), ["Router0", "PC0"]);
    }

    #[tokio::test]
    async fn adds_with_the_requested_name_and_position() {
        let (_canvas, packet_tracer) = canvas();
        let request = AddDeviceRequest {
            x: Some(300),
            y: Some(40),
            ..add_request("2960-24TT", Some("SW-CORE"))
        };
        let switch = add(&packet_tracer, &request).await.unwrap();
        assert_eq!(
            (
                switch.name.as_str(),
                switch.kind.as_str(),
                switch.x,
                switch.y
            ),
            ("SW-CORE", "switch", 300.0, 40.0)
        );
    }

    #[tokio::test]
    async fn refuses_duplicate_names_and_half_positions() {
        let (canvas, packet_tracer) = canvas();
        add(&packet_tracer, &add_request("2911", Some("R1")))
            .await
            .unwrap();

        let duplicate = add(&packet_tracer, &add_request("PC-PT", Some("R1"))).await;
        assert!(matches!(duplicate, Err(PtError::InvalidInput(_))));
        let half = AddDeviceRequest {
            x: Some(10),
            ..add_request("PC-PT", None)
        };
        assert!(matches!(
            add(&packet_tracer, &half).await,
            Err(PtError::InvalidInput(_))
        ));
        assert_eq!(canvas.device_names(), ["R1"]);
    }

    #[tokio::test]
    async fn unknown_models_suggest_alternatives() {
        let (_canvas, packet_tracer) = canvas();
        let error = add(&packet_tracer, &add_request("2960", None))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("did you mean 2960-24TT?"));
    }

    #[tokio::test]
    async fn renames_moves_lists_and_removes() {
        let (canvas, packet_tracer) = canvas();
        add(&packet_tracer, &add_request("2911", None))
            .await
            .unwrap();
        add(&packet_tracer, &add_request("PC-PT", None))
            .await
            .unwrap();

        let renamed = rename(
            &packet_tracer,
            &RenameRequest {
                name: "Router0".into(),
                new_name: "R-EDGE".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.name, "R-EDGE");

        let moved = relocate(
            &packet_tracer,
            &MoveRequest {
                name: "R-EDGE".into(),
                x: 500,
                y: 60,
            },
        )
        .await
        .unwrap();
        assert_eq!((moved.x, moved.y), (500.0, 60.0));

        let listing = list(&packet_tracer).await.unwrap();
        let names: Vec<_> = listing
            .devices
            .iter()
            .map(|device| device.name.as_str())
            .collect();
        assert_eq!(names, ["R-EDGE", "PC0"]);

        assert_eq!(remove(&packet_tracer, "PC0").await.unwrap().removed, "PC0");
        assert_eq!(canvas.device_names(), ["R-EDGE"]);
    }

    #[tokio::test]
    async fn operations_on_missing_devices_say_which_one() {
        let (_canvas, packet_tracer) = canvas();
        let error = remove(&packet_tracer, "Ghost").await.unwrap_err();
        assert_eq!(error, PtError::NotFound("device `Ghost`".into()));
        let rename_ghost = RenameRequest {
            name: "Ghost".into(),
            new_name: "Spirit".into(),
        };
        assert!(matches!(
            rename(&packet_tracer, &rename_ghost).await,
            Err(PtError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn rename_refuses_taken_names() {
        let (_canvas, packet_tracer) = canvas();
        add(&packet_tracer, &add_request("2911", Some("R1")))
            .await
            .unwrap();
        add(&packet_tracer, &add_request("2911", Some("R2")))
            .await
            .unwrap();
        let clash = RenameRequest {
            name: "R2".into(),
            new_name: "R1".into(),
        };
        assert!(matches!(
            rename(&packet_tracer, &clash).await,
            Err(PtError::InvalidInput(_))
        ));
    }
}
