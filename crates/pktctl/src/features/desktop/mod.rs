mod email;
mod files;
mod vpn;
mod web;

use std::time::Duration;

use ptmp::{Call, Event, Subscription, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use tokio::{sync::broadcast::error::RecvError, time::Instant};

pub use email::{
    EmailAccount, EmailAccountRequest, ReceiveRequest, Received, ReceivedMail, SendRequest, Sent,
    configure_email, receive_email, send_email,
};
pub use files::{FileAction, FileEntry, FilesRequest, FilesResult, host_files};
pub use vpn::{VpnAction, VpnRequest, VpnState, vpn_client};
pub use web::{BrowseRequest, Page, browse_web};

use crate::{
    features::{devices::describe, paths::device},
    packet_tracer::{Events, PacketTracer, PtError, expect_text, kinds::runs_ios},
    server::PktctlServer,
};

/// One app on an end device's Desktop, reached through the process behind it.
struct App<'a, P> {
    packet_tracer: &'a P,
    device: String,
    process: Call,
}

impl<'a, P: PacketTracer> App<'a, P> {
    async fn open(
        packet_tracer: &'a P,
        device_name: &str,
        process: &str,
        app: &str,
    ) -> Result<Self, PtError> {
        let device_name = device_name.trim();
        let target = describe(packet_tracer, device_name).await?;
        if runs_ios(&target.kind) {
            return Err(PtError::InvalidInput(format!(
                "`{device_name}` is a {}; the {app} is an app of PCs, laptops and servers",
                target.kind
            )));
        }
        let process_call = device(device_name).method("getProcess", [Value::string(process)]);
        packet_tracer
            .call(process_call.clone().method("getObjectUuid", []))
            .await
            .map_err(|error| match error {
                PtError::NotFound(_) => {
                    PtError::InvalidInput(format!("`{device_name}` has no {app}"))
                }
                other => other,
            })?;
        Ok(Self {
            packet_tracer,
            device: device_name.to_owned(),
            process: process_call,
        })
    }

    async fn call(&self, steps: Call) -> Result<Value, PtError> {
        self.packet_tracer.call(steps).await
    }

    fn at(&self, method: &str, args: impl IntoIterator<Item = Value>) -> Call {
        self.process.clone().method(method, args)
    }

    async fn uuid_of(&self, object: Call) -> Result<String, PtError> {
        let uuid = self.call(object.method("getObjectUuid", [])).await?;
        expect_text(&uuid, "object id")
    }
}

/// Events of one object, subscribed before the action that raises them.
struct Listener<'a, P: PacketTracer> {
    packet_tracer: &'a P,
    subscriptions: Vec<Subscription>,
    events: Events,
    object: String,
}

impl<'a, P: PacketTracer> Listener<'a, P> {
    async fn start(
        packet_tracer: &'a P,
        class: &str,
        object: &str,
        names: &[&str],
    ) -> Result<Self, PtError> {
        let subscriptions: Vec<Subscription> = names
            .iter()
            .map(|name| Subscription::to(class, object, *name))
            .collect();
        let mut events = None;
        for subscription in &subscriptions {
            let receiver = packet_tracer.subscribe(subscription.clone()).await?;
            events.get_or_insert(receiver);
        }
        let events =
            events.ok_or_else(|| PtError::InvalidInput("no events to listen to".into()))?;
        Ok(Self {
            packet_tracer,
            subscriptions,
            events,
            object: object.to_owned(),
        })
    }

    /// The next event of this object before `deadline`, or `None` when time runs out.
    async fn next(&mut self, deadline: Instant) -> Result<Option<Event>, PtError> {
        loop {
            let received = tokio::time::timeout_at(deadline, self.events.recv()).await;
            match received {
                Err(_) => return Ok(None),
                Ok(Ok(event)) if event.object_uuid == self.object => return Ok(Some(event)),
                Ok(Ok(_) | Err(RecvError::Lagged(_))) => {}
                Ok(Err(RecvError::Closed)) => {
                    return Err(PtError::Unreachable("the event stream closed".into()));
                }
            }
        }
    }

    async fn stop(self) {
        for subscription in self.subscriptions {
            if let Err(error) = self.packet_tracer.unsubscribe(subscription).await {
                tracing::debug!(%error, "could not unsubscribe from desktop app events");
            }
        }
    }
}

fn deadline(timeout: Duration) -> Instant {
    Instant::now() + timeout
}

fn text_arg(event: &Event, index: usize) -> String {
    match event.args.get(index) {
        Some(Value::Ip(address)) => address.to_string(),
        Some(Value::Ipv6(address)) => address.to_string(),
        Some(value) => value.as_str().unwrap_or_default().to_owned(),
        None => String::new(),
    }
}

fn code_arg(event: &Event, index: usize) -> i64 {
    event.args.get(index).and_then(Value::as_i64).unwrap_or(-1)
}

#[tool_router(router = desktop_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "browse_web",
        description = "Open a URL in the Web Browser of a PC, laptop or server, like typing it and \
                       pressing Go, and return the page Packet Tracer served: its status (ok, \
                       timeout, host_not_found, ...), the server's address, the HTML and the \
                       text. Names are resolved through the host's DNS server.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn browse_web_tool(
        &self,
        Parameters(request): Parameters<BrowseRequest>,
    ) -> Result<Json<Page>, String> {
        browse_web(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "configure_email",
        description = "Set up the Email app of a PC, laptop or server: display name, address, \
                       user name, password and the incoming (POP3) and outgoing (SMTP) servers. \
                       Returns the account as Packet Tracer now has it.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn configure_email_tool(
        &self,
        Parameters(request): Parameters<EmailAccountRequest>,
    ) -> Result<Json<EmailAccount>, String> {
        configure_email(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "send_email",
        description = "Send an email from the account set up with configure_email and wait for \
                       the SMTP server's answer. A recipient the server does not know comes back \
                       later as a delivery failure in receive_email.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn send_email_tool(
        &self,
        Parameters(request): Parameters<SendRequest>,
    ) -> Result<Json<Sent>, String> {
        send_email(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "receive_email",
        description = "Press Receive in the Email app: download the account's new mail over \
                       POP3 and return it. Like a real POP3 client, the mail is removed from the \
                       server; Packet Tracer does not expose the app's inbox, so keep what this \
                       returns.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn receive_email_tool(
        &self,
        Parameters(request): Parameters<ReceiveRequest>,
    ) -> Result<Json<Received>, String> {
        receive_email(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "vpn_client",
        description = "Use the VPN app of a PC, laptop or server against an Easy VPN server: \
                       `connect` with server, group, group_key, username and password and wait \
                       for the tunnel, `disconnect`, or `status`. Returns whether the tunnel is \
                       up and the address the server assigned.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn vpn_client_tool(
        &self,
        Parameters(request): Parameters<VpnRequest>,
    ) -> Result<Json<VpnState>, String> {
        vpn_client(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "host_files",
        description = "Work with the text files of a PC, laptop or server, the ones its Text \
                       Editor opens and `dir` lists: `list`, `read`, `write` (creates or \
                       replaces) or `delete` a file in C:\\.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn host_files_tool(
        &self,
        Parameters(request): Parameters<FilesRequest>,
    ) -> Result<Json<FilesResult>, String> {
        host_files(self.packet_tracer(), &request)
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
        features::{
            devices::{AddDeviceRequest, add},
            hosts::{HostConfigRequest, configure},
            services::{
                DnsRecord, DnsRequest, RecordType, UserRequest, UserService, WebPageRequest,
                add_user, configure_dns, set_web_page,
            },
        },
        packet_tracer::scripted::ScriptedPacketTracer,
        testing::Canvas,
    };

    const SERVER: &str = "192.168.10.10";

    async fn office() -> ScriptedPacketTracer {
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::new(Canvas::new()));
        for (model, name, ip) in [
            ("Server-PT", "SRV1", SERVER),
            ("PC-PT", "PC1", "192.168.10.20"),
        ] {
            add(
                &packet_tracer,
                &AddDeviceRequest {
                    model: model.into(),
                    name: Some(name.into()),
                    ..AddDeviceRequest::default()
                },
            )
            .await
            .unwrap();
            configure(
                &packet_tracer,
                &HostConfigRequest {
                    device: name.into(),
                    ip: Some(ip.into()),
                    mask: Some("255.255.255.0".into()),
                    dns: Some(SERVER.into()),
                    ..HostConfigRequest::default()
                },
            )
            .await
            .unwrap();
        }
        configure_dns(
            &packet_tracer,
            &DnsRequest {
                device: "SRV1".into(),
                records: vec![DnsRecord {
                    name: "www.gamc.bo".into(),
                    record_type: RecordType::A,
                    value: SERVER.into(),
                }],
                ..DnsRequest::default()
            },
        )
        .await
        .unwrap();
        set_web_page(
            &packet_tracer,
            &WebPageRequest {
                device: "SRV1".into(),
                url: "index.html".into(),
                contents: "<h1>GAMC</h1><p>Bienvenidos</p>".into(),
            },
        )
        .await
        .unwrap();
        for user in ["ana", "luis"] {
            add_user(
                &packet_tracer,
                &UserRequest {
                    device: "SRV1".into(),
                    service: UserService::Email,
                    username: user.into(),
                    password: "cisco".into(),
                    permissions: None,
                    domain: Some("gamc.bo".into()),
                },
            )
            .await
            .unwrap();
        }
        packet_tracer
    }

    fn browse(url: &str) -> BrowseRequest {
        BrowseRequest {
            device: "PC1".into(),
            url: url.into(),
            timeout_secs: Some(5),
        }
    }

    #[tokio::test]
    async fn browses_by_address_and_by_name() {
        let packet_tracer = office().await;
        let page = browse_web(&packet_tracer, &browse("www.gamc.bo"))
            .await
            .unwrap();
        assert_eq!(page.status, "ok");
        assert_eq!(page.server.as_deref(), Some(SERVER));
        assert_eq!(page.text, "GAMC\nBienvenidos");

        let missing = browse_web(&packet_tracer, &browse("http://192.168.10.10/nada.html"))
            .await
            .unwrap();
        assert_eq!(missing.status, "not_found");
        let unknown = browse_web(&packet_tracer, &browse("www.nadie.bo"))
            .await
            .unwrap();
        assert_eq!(
            (unknown.status.as_str(), unknown.server),
            ("host_not_found", None)
        );

        let router = browse_web(
            &packet_tracer,
            &BrowseRequest {
                device: "SRV9".into(),
                ..browse("www.gamc.bo")
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(router, PtError::NotFound(_)), "{router}");
    }

    fn account(device: &str, user: &str) -> EmailAccountRequest {
        EmailAccountRequest {
            device: device.into(),
            name: user.into(),
            email: format!("{user}@gamc.bo"),
            username: user.into(),
            password: "cisco".into(),
            incoming_server: SERVER.into(),
            outgoing_server: SERVER.into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sends_and_receives_mail_between_accounts() {
        let packet_tracer = office().await;
        let unconfigured = send_email(
            &packet_tracer,
            &SendRequest {
                device: "PC1".into(),
                to: "luis@gamc.bo".into(),
                ..SendRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            unconfigured.to_string().contains("configure_email"),
            "{unconfigured}"
        );

        let configured = configure_email(&packet_tracer, &account("PC1", "ana"))
            .await
            .unwrap();
        assert_eq!(configured.email, "ana@gamc.bo");
        let sent = send_email(
            &packet_tracer,
            &SendRequest {
                device: "PC1".into(),
                to: "luis@gamc.bo".into(),
                subject: "Informe".into(),
                body: "Adjunto el informe".into(),
                timeout_secs: Some(5),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (sent.from.as_str(), sent.to.as_str()),
            ("ana@gamc.bo", "luis@gamc.bo")
        );

        configure_email(&packet_tracer, &account("SRV1", "luis"))
            .await
            .unwrap();
        let receive = ReceiveRequest {
            device: "SRV1".into(),
            timeout_secs: Some(1),
        };
        let inbox = receive_email(&packet_tracer, &receive).await.unwrap();
        assert_eq!(inbox.mails.len(), 1);
        assert_eq!(inbox.mails[0].from, "ana@gamc.bo");
        assert_eq!(inbox.mails[0].body, "Adjunto el informe");
        let again = receive_email(&packet_tracer, &receive).await.unwrap();
        assert!(again.mails.is_empty(), "POP3 removes what it hands over");

        let mut wrong = account("SRV1", "luis");
        wrong.password = "mala".into();
        configure_email(&packet_tracer, &wrong).await.unwrap();
        let refused = receive_email(&packet_tracer, &receive).await.unwrap_err();
        assert!(
            refused.to_string().contains("wrong user name or password"),
            "{refused}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn connects_and_disconnects_the_vpn_client() {
        let packet_tracer = office().await;
        let connect = |server: &str| VpnRequest {
            device: "PC1".into(),
            action: VpnAction::Connect,
            server: Some(server.into()),
            group: Some("VPNGROUP".into()),
            group_key: Some("vpnkey".into()),
            username: Some("vpnuser".into()),
            password: Some("vpnpass".into()),
            timeout_secs: Some(2),
        };
        let up = vpn_client(&packet_tracer, &connect(SERVER)).await.unwrap();
        assert!(up.connected);
        assert_eq!(up.tunnel_ip.as_deref(), Some("10.50.0.10"));
        assert_eq!(up.group.as_deref(), Some("VPNGROUP"));

        let down = vpn_client(
            &packet_tracer,
            &VpnRequest {
                device: "PC1".into(),
                action: VpnAction::Disconnect,
                ..VpnRequest::default()
            },
        )
        .await
        .unwrap();
        assert!(!down.connected && down.tunnel_ip.is_none());

        let nobody = vpn_client(&packet_tracer, &connect("192.168.10.99"))
            .await
            .unwrap_err();
        assert!(
            nobody.to_string().contains("did not bring the tunnel up"),
            "{nobody}"
        );
        let incomplete = vpn_client(
            &packet_tracer,
            &VpnRequest {
                group_key: None,
                ..connect(SERVER)
            },
        )
        .await
        .unwrap_err();
        assert!(incomplete.to_string().contains("group_key"), "{incomplete}");
    }

    #[tokio::test]
    async fn reads_writes_and_deletes_text_files() {
        let packet_tracer = office().await;
        let files = |action, name: Option<&str>, text: Option<&str>| FilesRequest {
            device: "PC1".into(),
            action,
            name: name.map(Into::into),
            text: text.map(Into::into),
        };
        let listed = host_files(&packet_tracer, &files(FileAction::List, None, None))
            .await
            .unwrap();
        assert_eq!(listed.files.len(), 1);

        host_files(
            &packet_tracer,
            &files(FileAction::Write, Some("notas.txt"), Some("VLAN 10")),
        )
        .await
        .unwrap();
        host_files(
            &packet_tracer,
            &files(FileAction::Write, Some("notas.txt"), Some("VLAN 20")),
        )
        .await
        .unwrap();
        let read = host_files(
            &packet_tracer,
            &files(FileAction::Read, Some("notas.txt"), None),
        )
        .await
        .unwrap();
        assert_eq!(read.text.as_deref(), Some("VLAN 20"));
        assert!(
            read.files
                .iter()
                .any(|file| file.name == "notas.txt" && file.size == 7)
        );

        let deleted = host_files(
            &packet_tracer,
            &files(FileAction::Delete, Some("notas.txt"), None),
        )
        .await
        .unwrap();
        assert_eq!(deleted.files.len(), 1);
        let gone = host_files(
            &packet_tracer,
            &files(FileAction::Read, Some("notas.txt"), None),
        )
        .await
        .unwrap_err();
        assert!(matches!(gone, PtError::NotFound(_)), "{gone}");
        assert!(
            host_files(
                &packet_tracer,
                &files(FileAction::Write, Some("a/b.txt"), Some("x"))
            )
            .await
            .is_err()
        );
    }
}
