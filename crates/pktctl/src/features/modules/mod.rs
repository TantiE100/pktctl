mod install;
mod slots;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};

pub use install::{AddModuleRequest, ModuleChange, RemoveModuleRequest, add_module, remove_module};
pub use slots::{Slot, SlotList, list_slots};

use crate::{features::links::PortsRequest, packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = modules_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_slots",
        description = "List a device's module slots (path, the kind of module each accepts, what \
                       is installed) and the module models the device supports.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_slots_tool(
        &self,
        Parameters(request): Parameters<PortsRequest>,
    ) -> Result<Json<SlotList>, String> {
        list_slots(self.packet_tracer(), &request.device)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_module",
        description = "Install a module (for example HWIC-2T for serial ports) in an empty slot. \
                       The device is powered off, the module inserted and the device powered back \
                       on, exactly like real hardware, so unsaved configuration is lost. Returns \
                       the ports the module added.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn add_module_tool(
        &self,
        Parameters(request): Parameters<AddModuleRequest>,
    ) -> Result<Json<ModuleChange>, String> {
        add_module(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "remove_module",
        description = "Remove the module in a slot, power cycling the device like add_module. \
                       Returns the ports that disappeared with it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn remove_module_tool(
        &self,
        Parameters(request): Parameters<RemoveModuleRequest>,
    ) -> Result<Json<ModuleChange>, String> {
        remove_module(self.packet_tracer(), &request)
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
        packet_tracer::{PtError, scripted::ScriptedPacketTracer},
        testing::Canvas,
    };

    async fn router() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        let request = AddDeviceRequest {
            model: "2911".into(),
            name: Some("R1".into()),
            ..AddDeviceRequest::default()
        };
        add(&packet_tracer, &request).await.unwrap();
        (canvas, packet_tracer)
    }

    fn install(slot: &str, module: &str) -> AddModuleRequest {
        AddModuleRequest {
            device: "R1".into(),
            slot: slot.into(),
            module: module.into(),
        }
    }

    #[tokio::test]
    async fn lists_removable_slots_and_supported_modules() {
        let (_canvas, packet_tracer) = router().await;
        let inventory = list_slots(&packet_tracer, "R1").await.unwrap();
        let slots: Vec<_> = inventory
            .slots
            .iter()
            .map(|slot| {
                (
                    slot.path.as_str(),
                    slot.accepts.as_str(),
                    slot.installed.clone(),
                )
            })
            .collect();
        assert_eq!(
            slots,
            ["0/0", "0/1", "0/2", "0/3"].map(|path| (path, "interface_card", None))
        );
        assert_eq!(inventory.supported_modules, ["HWIC-2T"]);
    }

    #[tokio::test]
    async fn installs_with_a_power_cycle_and_reports_new_ports() {
        let (canvas, packet_tracer) = router().await;
        let change = add_module(&packet_tracer, &install("0/1", "hwic-2t"))
            .await
            .unwrap();
        assert_eq!(
            change,
            ModuleChange {
                device: "R1".into(),
                slot: "0/1".into(),
                module: "HWIC-2T".into(),
                ports_added: vec!["Serial0/1/0".into(), "Serial0/1/1".into()],
                ports_removed: Vec::new(),
                cut_links: Vec::new(),
            }
        );
        assert_eq!(canvas.installed_cards("R1")[1].as_deref(), Some("HWIC-2T"));
        assert_eq!(canvas.is_powered("R1"), Some(true));
        assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router>"));

        let inventory = list_slots(&packet_tracer, "R1").await.unwrap();
        assert_eq!(inventory.slots[1].installed.as_deref(), Some("HWIC-2T"));
    }

    #[tokio::test]
    async fn removes_and_reports_lost_ports() {
        let (canvas, packet_tracer) = router().await;
        add_module(&packet_tracer, &install("0/2", "HWIC-2T"))
            .await
            .unwrap();
        let change = remove_module(
            &packet_tracer,
            &RemoveModuleRequest {
                device: "R1".into(),
                slot: "0/2".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(change.ports_removed, ["Serial0/2/0", "Serial0/2/1"]);
        assert!(canvas.installed_cards("R1").iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn removing_a_cabled_module_reports_the_cut_links() {
        let (canvas, packet_tracer) = router().await;
        let second = AddDeviceRequest {
            model: "2911".into(),
            name: Some("R2".into()),
            ..AddDeviceRequest::default()
        };
        add(&packet_tracer, &second).await.unwrap();
        add_module(&packet_tracer, &install("0/0", "HWIC-2T"))
            .await
            .unwrap();
        add_module(
            &packet_tracer,
            &AddModuleRequest {
                device: "R2".into(),
                ..install("0/0", "HWIC-2T")
            },
        )
        .await
        .unwrap();
        crate::features::links::connect(
            &packet_tracer,
            &crate::features::links::ConnectRequest {
                device_a: "R1".into(),
                port_a: "Serial0/0/0".into(),
                device_b: "R2".into(),
                port_b: "Serial0/0/0".into(),
                cable: crate::features::links::Cable::Auto,
            },
        )
        .await
        .unwrap();

        let change = remove_module(
            &packet_tracer,
            &RemoveModuleRequest {
                device: "R1".into(),
                slot: "0/0".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(change.cut_links.len(), 1);
        assert_eq!(change.cut_links[0].b.device, "R2");
        assert_eq!(change.cut_links[0].cable, "serial");
        assert!(canvas.links().is_empty());
    }

    #[tokio::test]
    async fn validates_before_touching_the_power() {
        let (canvas, packet_tracer) = router().await;
        let unsupported = add_module(&packet_tracer, &install("0/1", "NIM-2T"))
            .await
            .unwrap_err();
        assert!(
            unsupported.to_string().contains("it supports HWIC-2T"),
            "{unsupported}"
        );

        let missing_slot = add_module(&packet_tracer, &install("0/9", "HWIC-2T"))
            .await
            .unwrap_err();
        assert!(
            missing_slot
                .to_string()
                .contains("its slots are 0/0, 0/1, 0/2, 0/3")
        );

        add_module(&packet_tracer, &install("0/0", "HWIC-2T"))
            .await
            .unwrap();
        let occupied = add_module(&packet_tracer, &install("0/0", "HWIC-2T"))
            .await
            .unwrap_err();
        assert!(occupied.to_string().contains("already holds HWIC-2T"));

        let empty = remove_module(
            &packet_tracer,
            &RemoveModuleRequest {
                device: "R1".into(),
                slot: "0/3".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(empty, PtError::InvalidInput(_)));
        assert_eq!(canvas.is_powered("R1"), Some(true));
    }
}
