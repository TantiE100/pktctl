mod radio;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};

pub use radio::{
    AccessPointConfig, AccessPointRequest, ConnectWirelessRequest, Security, StatusRequest,
    WirelessConnection, WirelessStatus, configure_access_point, connect_wireless, status,
};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = wireless_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "configure_access_point",
        description = "Set the SSID, security (open, wep, wpa_psk, wpa2_psk), key and SSID \
                       broadcast of an access point or wireless router. Clients keep their \
                       current association until connect_wireless reconnects them.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn configure_access_point_tool(
        &self,
        Parameters(request): Parameters<AccessPointRequest>,
    ) -> Result<Json<AccessPointConfig>, String> {
        configure_access_point(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "connect_wireless",
        description = "Connect a laptop, PC or other client with a wireless card to a network: \
                       SSID, security and key, plus DHCP or a static address. Packet Tracer \
                       only associates when a network is loaded, so pktctl saves the network, \
                       sets the client's current profile in the file, reopens it and reports \
                       whether it associated and with which access point.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn connect_wireless_tool(
        &self,
        Parameters(request): Parameters<ConnectWirelessRequest>,
    ) -> Result<Json<WirelessConnection>, String> {
        connect_wireless(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "wireless_status",
        description = "Read the wireless settings of an access point, wireless router or \
                       client, and for clients which access point they are associated with.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn wireless_status_tool(
        &self,
        Parameters(request): Parameters<StatusRequest>,
    ) -> Result<Json<WirelessStatus>, String> {
        status(self.packet_tracer(), &request)
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

    async fn lab() -> ScriptedPacketTracer {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(canvas);
        for (model, name) in [
            ("AccessPoint-PT", "AP"),
            ("Laptop-PT", "LT"),
            ("PC-PT", "PC"),
        ] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        packet_tracer
    }

    fn secure_ap(key: &str) -> AccessPointRequest {
        AccessPointRequest {
            device: "AP".into(),
            ssid: "GAMC".into(),
            security: Security::Wpa2Psk,
            key: Some(key.into()),
            broadcast_ssid: None,
        }
    }

    fn join(key: &str) -> ConnectWirelessRequest {
        ConnectWirelessRequest {
            device: "LT".into(),
            ssid: "GAMC".into(),
            security: Security::Wpa2Psk,
            key: Some(key.into()),
            bring_access_point: false,
            ip: Some("192.168.50.20".into()),
            mask: Some("255.255.255.0".into()),
            gateway: Some("192.168.50.1".into()),
            dns: None,
        }
    }

    #[tokio::test]
    async fn connects_with_the_right_key_and_addresses_the_radio() {
        let packet_tracer = lab().await;
        let ap = configure_access_point(&packet_tracer, &secure_ap("clave1234"))
            .await
            .unwrap();
        assert_eq!(
            (ap.ssid.as_str(), ap.security, ap.broadcast_ssid),
            ("GAMC", Some(Security::Wpa2Psk), true)
        );

        let connection = connect_wireless(&packet_tracer, &join("clave1234"))
            .await
            .unwrap();
        assert!(connection.associated);
        assert_eq!(connection.access_point.as_deref(), Some("AP"));
        assert_eq!(connection.ip.as_deref(), Some("192.168.50.20"));

        let client = status(
            &packet_tracer,
            &StatusRequest {
                device: "LT".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (client.role.as_str(), client.access_point.as_deref()),
            ("client", Some("AP"))
        );
        std::fs::remove_file(connection.file).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_wrong_key_does_not_associate() {
        let packet_tracer = lab().await;
        configure_access_point(&packet_tracer, &secure_ap("clave1234"))
            .await
            .unwrap();
        let connection = connect_wireless(&packet_tracer, &join("incorrecta"))
            .await
            .unwrap();
        assert!(!connection.associated);
        assert!(connection.access_point.is_none());
        std::fs::remove_file(connection.file).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn explains_range_and_brings_the_access_point_when_asked() {
        let packet_tracer = lab().await;
        configure_access_point(&packet_tracer, &secure_ap("clave1234"))
            .await
            .unwrap();
        crate::features::physical::move_to_location(
            &packet_tracer,
            &crate::features::physical::MoveRequest {
                device: Some("AP".into()),
                into: "Home City/Corporate Office".into(),
                x: Some(900),
                y: Some(0),
                ..crate::features::physical::MoveRequest::default()
            },
        )
        .await
        .unwrap();

        let far = connect_wireless(&packet_tracer, &join("clave1234"))
            .await
            .unwrap();
        assert!(!far.associated);
        let diagnosis = far.diagnosis.unwrap();
        assert!(diagnosis.contains("AP is 900 units away"), "{diagnosis}");
        assert!(diagnosis.contains("bring_access_point"));

        let brought = connect_wireless(
            &packet_tracer,
            &ConnectWirelessRequest {
                bring_access_point: true,
                ..join("clave1234")
            },
        )
        .await
        .unwrap();
        assert!(brought.associated);
        assert_eq!(brought.moved_access_point.as_deref(), Some("AP"));
        std::fs::remove_file(brought.file).unwrap();

        let wrong = connect_wireless(&packet_tracer, &join("incorrecta"))
            .await
            .unwrap();
        assert!(wrong.diagnosis.unwrap().contains("check the security"));
        std::fs::remove_file(wrong.file).unwrap();
    }

    #[tokio::test]
    async fn explains_devices_and_keys_that_cannot_work() {
        let packet_tracer = lab().await;
        let wired = connect_wireless(
            &packet_tracer,
            &ConnectWirelessRequest {
                device: "PC".into(),
                ..join("clave1234")
            },
        )
        .await
        .unwrap_err();
        assert!(wired.to_string().contains("no wireless card"), "{wired}");

        let not_ap = configure_access_point(
            &packet_tracer,
            &AccessPointRequest {
                device: "LT".into(),
                ..secure_ap("clave1234")
            },
        )
        .await
        .unwrap_err();
        assert!(not_ap.to_string().contains("not an access point"));

        for (security, key) in [
            (Security::Wpa2Psk, "short"),
            (Security::Wep, "xyz"),
            (Security::Open, "extra"),
        ] {
            let error = configure_access_point(
                &packet_tracer,
                &AccessPointRequest {
                    security,
                    key: Some(key.into()),
                    ..secure_ap("clave1234")
                },
            )
            .await
            .unwrap_err();
            assert!(matches!(error, PtError::InvalidInput(_)), "{security:?}");
        }
    }
}
