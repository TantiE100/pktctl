mod catalog;
mod setup;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};

pub use catalog::Service;
pub use setup::{
    DhcpPool, DhcpRequest, DhcpServer, DnsRecord, DnsRequest, DnsServer, PoolRequest, RecordType,
    ServiceState, ServicesList, ServicesRequest, SetServiceRequest, UserAdded, UserRequest,
    UserService, WebPage, WebPageRequest, add_user, configure_dhcp, configure_dns, list_services,
    set_service, set_web_page,
};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = services_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_server_services",
        description = "List the services of a server (DHCP, DNS, HTTP, HTTPS, FTP, SMTP, POP3, \
                       NTP, Syslog, TFTP) and whether each one is on.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_server_services_tool(
        &self,
        Parameters(request): Parameters<ServicesRequest>,
    ) -> Result<Json<ServicesList>, String> {
        list_services(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "set_server_service",
        description = "Switch one service of a server on or off, as in its Services tab.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn set_server_service_tool(
        &self,
        Parameters(request): Parameters<SetServiceRequest>,
    ) -> Result<Json<ServiceState>, String> {
        set_service(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "configure_dhcp_server",
        description = "Configure the DHCP service of a server: switch it on and create or \
                       update pools (gateway, DNS, start address, mask, maximum users). \
                       Existing pools with the same name are updated.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn configure_dhcp_server_tool(
        &self,
        Parameters(request): Parameters<DhcpRequest>,
    ) -> Result<Json<DhcpServer>, String> {
        configure_dhcp(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "configure_dns_server",
        description = "Switch a server's DNS service on and add A, CNAME or NS records.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn configure_dns_server_tool(
        &self,
        Parameters(request): Parameters<DnsRequest>,
    ) -> Result<Json<DnsServer>, String> {
        configure_dns(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "set_web_page",
        description = "Write a page of a server's HTTP service, for example index.html.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn set_web_page_tool(
        &self,
        Parameters(request): Parameters<WebPageRequest>,
    ) -> Result<Json<WebPage>, String> {
        set_web_page(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_server_user",
        description = "Add an FTP account (with permissions such as RWDNL) or an email account \
                       to a server.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_server_user_tool(
        &self,
        Parameters(request): Parameters<UserRequest>,
    ) -> Result<Json<UserAdded>, String> {
        add_user(self.packet_tracer(), &request)
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
        for (model, name) in [("Server-PT", "SRV"), ("PC-PT", "PC")] {
            let request = AddDeviceRequest {
                model: model.into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        packet_tracer
    }

    fn server() -> ServicesRequest {
        ServicesRequest {
            device: "SRV".into(),
            port: None,
        }
    }

    #[tokio::test]
    async fn lists_and_switches_services() {
        let packet_tracer = lab().await;
        let listed = list_services(&packet_tracer, &server()).await.unwrap();
        assert_eq!(listed.services.len(), 10);
        assert_eq!(
            listed.services[0],
            ServiceState {
                service: Service::Dhcp,
                enabled: false
            }
        );
        let off = set_service(
            &packet_tracer,
            &SetServiceRequest {
                device: "SRV".into(),
                service: Service::Ftp,
                enabled: false,
                port: None,
            },
        )
        .await
        .unwrap();
        assert!(!off.enabled);
        let https = set_service(
            &packet_tracer,
            &SetServiceRequest {
                device: "SRV".into(),
                service: Service::Https,
                enabled: false,
                port: None,
            },
        )
        .await
        .unwrap();
        assert!(!https.enabled);
    }

    #[tokio::test]
    async fn updates_the_default_pool_and_adds_new_ones() {
        let packet_tracer = lab().await;
        let pool = |name: &str, third: u8, max_users| PoolRequest {
            name: name.into(),
            gateway: format!("192.168.{third}.1"),
            start_ip: format!("192.168.{third}.100"),
            mask: "255.255.255.0".into(),
            dns: Some("192.168.10.5".into()),
            max_users,
        };
        let dhcp = configure_dhcp(
            &packet_tracer,
            &DhcpRequest {
                device: "SRV".into(),
                pools: vec![pool("serverPool", 10, Some(50)), pool("VLAN20", 20, None)],
                ..DhcpRequest::default()
            },
        )
        .await
        .unwrap();
        assert!(dhcp.enabled);
        assert_eq!(dhcp.pools.len(), 2);
        let first = &dhcp.pools[0];
        assert_eq!(
            (
                first.network.as_str(),
                first.gateway.as_str(),
                first.end_ip.as_str(),
                first.max_users
            ),
            ("192.168.10.0", "192.168.10.1", "192.168.10.149", 50)
        );
        assert_eq!(dhcp.pools[1].name, "VLAN20");

        let bad = configure_dhcp(
            &packet_tracer,
            &DhcpRequest {
                device: "SRV".into(),
                pools: vec![PoolRequest {
                    gateway: "not-an-ip".into(),
                    ..pool("X", 30, None)
                }],
                ..DhcpRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad, PtError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn adds_dns_records_pages_and_users() {
        let packet_tracer = lab().await;
        let dns = configure_dns(
            &packet_tracer,
            &DnsRequest {
                device: "SRV".into(),
                enabled: None,
                records: vec![
                    DnsRecord {
                        name: "www.gamc.bo".into(),
                        record_type: RecordType::A,
                        value: "192.168.10.5".into(),
                    },
                    DnsRecord {
                        name: "portal.gamc.bo".into(),
                        record_type: RecordType::Cname,
                        value: "www.gamc.bo".into(),
                    },
                ],
            },
        )
        .await
        .unwrap();
        assert!(dns.enabled);
        assert_eq!(dns.records[0].value, "192.168.10.5");
        assert_eq!(
            (
                dns.records[1].record_type.as_str(),
                dns.records[1].value.as_str()
            ),
            ("CNAME", "www.gamc.bo")
        );

        let page = set_web_page(
            &packet_tracer,
            &WebPageRequest {
                device: "SRV".into(),
                url: "index.html".into(),
                contents: "<h1>GAMC</h1>".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(page.bytes, 13);

        for service in [UserService::Ftp, UserService::Email] {
            let added = add_user(
                &packet_tracer,
                &UserRequest {
                    device: "SRV".into(),
                    service,
                    username: "ana".into(),
                    password: "ana123".into(),
                    permissions: Some("rwl".into()),
                    domain: Some("gamc.bo".into()),
                },
            )
            .await
            .unwrap();
            assert_eq!(added.username, "ana");
        }
        let bad_permissions = add_user(
            &packet_tracer,
            &UserRequest {
                device: "SRV".into(),
                service: UserService::Ftp,
                username: "bob".into(),
                password: "x".into(),
                permissions: Some("RX".into()),
                domain: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(bad_permissions, PtError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn refuses_devices_without_services() {
        let packet_tracer = lab().await;
        let error = list_services(
            &packet_tracer,
            &ServicesRequest {
                device: "PC".into(),
                port: None,
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("no server services"), "{error}");
    }
}
