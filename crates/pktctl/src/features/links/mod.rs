mod cabling;
mod connect;
mod ports;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

pub use cabling::Cable;
pub use connect::{ConnectRequest, Disconnected, PortRef, connect, disconnect};
pub use ports::{
    Connection, Endpoint, Link, LinkList, Port, PortList, connection, list_links, list_ports,
};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PortsRequest {
    /// Device name as shown by `list_devices`.
    pub device: String,
}

#[tool_router(router = links_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_ports",
        description = "List a device's ports with their state, IPv4 address when they have one, \
                       and what each port is connected to. Use it to pick free ports before \
                       connect.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_ports_tool(
        &self,
        Parameters(request): Parameters<PortsRequest>,
    ) -> Result<Json<PortList>, String> {
        list_ports(self.packet_tracer(), &request.device)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "list_links",
        description = "List every cable in the network with both endpoints and the cable type.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_links_tool(&self) -> Result<Json<LinkList>, String> {
        list_links(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "connect",
        description = "Cable two ports together. Both ports must exist and be free. With the \
                       default `auto` cable pktctl picks serial, straight or cross the way the \
                       CCNA rules do; pass `cable` to force fiber, rollover, console, etc.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn connect_tool(
        &self,
        Parameters(request): Parameters<ConnectRequest>,
    ) -> Result<Json<Link>, String> {
        connect(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "disconnect",
        description = "Remove the cable plugged into a port.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn disconnect_tool(
        &self,
        Parameters(request): Parameters<PortRef>,
    ) -> Result<Json<Disconnected>, String> {
        disconnect(self.packet_tracer(), &request)
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

    async fn network() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        for (model, name) in [("2911", "R1"), ("2960-24TT", "SW1"), ("PC-PT", "PC1")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        (canvas, packet_tracer)
    }

    fn cable(device_a: &str, port_a: &str, device_b: &str, port_b: &str) -> ConnectRequest {
        ConnectRequest {
            device_a: device_a.into(),
            port_a: port_a.into(),
            device_b: device_b.into(),
            port_b: port_b.into(),
            cable: Cable::Auto,
        }
    }

    fn endpoint(device: &str, port: &str) -> Endpoint {
        Endpoint {
            device: device.into(),
            port: port.into(),
        }
    }

    #[tokio::test]
    async fn wireless_associations_are_not_cables() {
        let (canvas, packet_tracer) = network().await;
        connect(
            &packet_tracer,
            &cable("R1", "GigabitEthernet0/0", "SW1", "GigabitEthernet0/1"),
        )
        .await
        .unwrap();
        canvas.associate_wireless("PC1", "FastEthernet0");

        let links = list_links(&packet_tracer).await.unwrap();
        assert_eq!(links.links.len(), 1);
        let ports = list_ports(&packet_tracer, "PC1").await.unwrap();
        let radio = &ports.ports[0];
        assert!(radio.wireless);
        assert!(radio.connection.is_none());
    }

    #[tokio::test]
    async fn auto_cabling_picks_straight_between_layers() {
        let (canvas, packet_tracer) = network().await;
        let link = connect(
            &packet_tracer,
            &cable("PC1", "FastEthernet0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap();
        assert_eq!(
            link,
            Link {
                a: endpoint("PC1", "FastEthernet0"),
                b: endpoint("SW1", "FastEthernet0/1"),
                cable: "straight".into(),
            }
        );
        assert_eq!(canvas.links()[0].cable, 8100);
    }

    #[tokio::test]
    async fn ports_show_state_addresses_and_peers() {
        let (_canvas, packet_tracer) = network().await;
        connect(
            &packet_tracer,
            &cable("R1", "GigabitEthernet0/0", "SW1", "GigabitEthernet0/1"),
        )
        .await
        .unwrap();

        let router = list_ports(&packet_tracer, "R1").await.unwrap();
        let names: Vec<_> = router.ports.iter().map(|port| port.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Vlan1",
                "GigabitEthernet0/0",
                "GigabitEthernet0/1",
                "GigabitEthernet0/2"
            ]
        );
        let uplink = &router.ports[1];
        assert!(uplink.up && uplink.protocol_up);
        assert_eq!(
            uplink.connection,
            Some(Connection {
                to: endpoint("SW1", "GigabitEthernet0/1"),
                cable: "straight".into(),
            })
        );
        assert_eq!(router.ports[2].connection, None);

        let switch = list_ports(&packet_tracer, "SW1").await.unwrap();
        assert!(switch.ports.iter().all(|port| port.ip.is_none()));
        let uplink = switch
            .ports
            .iter()
            .find(|port| port.name == "GigabitEthernet0/1")
            .unwrap();
        assert_eq!(
            uplink.connection.as_ref().unwrap().to,
            endpoint("R1", "GigabitEthernet0/0")
        );
    }

    #[tokio::test]
    async fn refuses_busy_ports_and_names_the_existing_peer() {
        let (_canvas, packet_tracer) = network().await;
        connect(
            &packet_tracer,
            &cable("PC1", "FastEthernet0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap();
        let error = connect(
            &packet_tracer,
            &cable("R1", "GigabitEthernet0/0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            PtError::InvalidInput(
                "SW1:FastEthernet0/1 is already connected to PC1:FastEthernet0".into()
            )
        );
    }

    #[tokio::test]
    async fn unknown_ports_list_the_real_ones() {
        let (_canvas, packet_tracer) = network().await;
        let error = connect(
            &packet_tracer,
            &cable("PC1", "Eth0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("its ports are FastEthernet0, Bluetooth")
        );
    }

    #[tokio::test]
    async fn explains_refused_media() {
        let (_canvas, packet_tracer) = network().await;
        let request = ConnectRequest {
            cable: Cable::Serial,
            ..cable("R1", "GigabitEthernet0/0", "SW1", "GigabitEthernet0/1")
        };
        let error = connect(&packet_tracer, &request).await.unwrap_err();
        assert!(error.to_string().contains("no serial cable fits"));
    }

    #[tokio::test]
    async fn wireless_ports_take_no_cable() {
        let (_canvas, packet_tracer) = network().await;
        let error = connect(
            &packet_tracer,
            &cable("PC1", "Bluetooth", "R1", "GigabitEthernet0/2"),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PtError::Rejected(_)));
    }

    #[tokio::test]
    async fn lists_links_and_disconnects_either_end() {
        let (canvas, packet_tracer) = network().await;
        connect(
            &packet_tracer,
            &cable("PC1", "FastEthernet0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap();
        connect(
            &packet_tracer,
            &cable("R1", "GigabitEthernet0/0", "SW1", "GigabitEthernet0/1"),
        )
        .await
        .unwrap();
        assert_eq!(list_links(&packet_tracer).await.unwrap().links.len(), 2);

        let gone = disconnect(
            &packet_tracer,
            &PortRef {
                device: "SW1".into(),
                port: "FastEthernet0/1".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(gone.was_connected_to, endpoint("PC1", "FastEthernet0"));
        assert_eq!(canvas.links().len(), 1);

        let again = disconnect(
            &packet_tracer,
            &PortRef {
                device: "SW1".into(),
                port: "FastEthernet0/1".into(),
            },
        )
        .await;
        assert!(matches!(again, Err(PtError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn links_follow_renamed_and_removed_devices() {
        let (canvas, packet_tracer) = network().await;
        connect(
            &packet_tracer,
            &cable("PC1", "FastEthernet0", "SW1", "FastEthernet0/1"),
        )
        .await
        .unwrap();
        crate::features::devices::rename(
            &packet_tracer,
            &crate::features::devices::RenameRequest {
                name: "PC1".into(),
                new_name: "PC-ADMIN".into(),
            },
        )
        .await
        .unwrap();
        let links = list_links(&packet_tracer).await.unwrap().links;
        assert_eq!(links[0].a, endpoint("PC-ADMIN", "FastEthernet0"));

        crate::features::devices::remove(&packet_tracer, "PC-ADMIN")
            .await
            .unwrap();
        assert!(canvas.links().is_empty());
    }
}
