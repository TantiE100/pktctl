use std::{net::Ipv4Addr, time::Duration};

use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::App;
use crate::{
    features::terminal::timeout,
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_text},
};

const PROCESS: &str = "EasyVpnClient";
const DEFAULT_TIMEOUT_SECS: u64 = 15;
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VpnAction {
    /// Report whether the tunnel is up and its address.
    #[default]
    Status,
    /// Fill in the VPN app and press Connect.
    Connect,
    /// Press Disconnect.
    Disconnect,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct VpnRequest {
    /// PC, laptop or server whose VPN app to use.
    pub device: String,
    /// `status` (default), `connect` or `disconnect`.
    #[serde(default)]
    pub action: VpnAction,
    /// Connect: Host IP, the Easy VPN server's address.
    #[serde(default)]
    pub server: Option<String>,
    /// Connect: the group name, as in `crypto isakmp client configuration group`.
    #[serde(default)]
    pub group: Option<String>,
    /// Connect: Group Key, the group's `key`.
    #[serde(default)]
    pub group_key: Option<String>,
    /// Connect: user name the server authenticates.
    #[serde(default)]
    pub username: Option<String>,
    /// Connect: that user's password.
    #[serde(default)]
    pub password: Option<String>,
    /// Seconds to wait for the tunnel. Defaults to 15, maximum 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct VpnState {
    pub device: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The address the server assigned inside the tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_ip: Option<String>,
}

pub async fn vpn_client<P: PacketTracer>(
    packet_tracer: &P,
    request: &VpnRequest,
) -> Result<VpnState, PtError> {
    let wait = timeout(Some(request.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)))?;
    let settings = match request.action {
        VpnAction::Connect => Some(settings(request)?),
        _ => None,
    };
    let app = App::open(packet_tracer, &request.device, PROCESS, "VPN app").await?;
    if let Some((server, fields)) = settings {
        app.call(app.at("setServerIp", [Value::Ip(server)])).await?;
        for (setter, value) in fields {
            app.call(app.at(setter, [Value::string(value)])).await?;
        }
        app.call(app.at("connect", [])).await?;
        if !wait_until(&app, true, wait).await? {
            return Err(PtError::Rejected(format!(
                "the VPN server {server} did not bring the tunnel up within {} seconds; check \
                 the group, key, user and password, and that the server answers (crypto map on \
                 its interface)",
                wait.as_secs()
            )));
        }
    } else if request.action == VpnAction::Disconnect {
        app.call(app.at("disconnect", [])).await?;
        wait_until(&app, false, wait).await?;
    }
    state(&app).await
}

type Settings = (Ipv4Addr, Vec<(&'static str, String)>);

fn settings(request: &VpnRequest) -> Result<Settings, PtError> {
    let field = |name: &str, value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| PtError::InvalidInput(format!("{name} is required to connect")))
    };
    let server = field("server", &request.server)?;
    let server: Ipv4Addr = server
        .parse()
        .map_err(|_| PtError::InvalidInput(format!("server `{server}` is not an IPv4 address")))?;
    Ok((
        server,
        vec![
            ("setGroupName", field("group", &request.group)?),
            ("setGroupKey", field("group_key", &request.group_key)?),
            ("setUsername", field("username", &request.username)?),
            ("setPassword", field("password", &request.password)?),
        ],
    ))
}

async fn wait_until<P: PacketTracer>(
    app: &App<'_, P>,
    connected: bool,
    wait: Duration,
) -> Result<bool, PtError> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let now = app.call(app.at("isConnected", [])).await?;
        if expect_bool(&now, "isConnected")? == connected {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn state<P: PacketTracer>(app: &App<'_, P>) -> Result<VpnState, PtError> {
    let (connected, server, group, username, tunnel) = tokio::try_join!(
        app.call(app.at("isConnected", [])),
        app.call(app.at("getServerIp", [])),
        app.call(app.at("getGroupName", [])),
        app.call(app.at("getUsername", [])),
        app.call(app.at("getTunnelIp", [])),
    )?;
    let address = |value: &Value| {
        value
            .as_ip()
            .filter(|address| !address.is_unspecified())
            .map(|address| address.to_string())
    };
    let text = |value: &Value, what: &str| {
        expect_text(value, what).map(|text| Some(text).filter(|text| !text.is_empty()))
    };
    Ok(VpnState {
        device: app.device.clone(),
        connected: expect_bool(&connected, "isConnected")?,
        server: address(&server),
        group: text(&group, "group name")?,
        username: text(&username, "user name")?,
        tunnel_ip: address(&tunnel),
    })
}
