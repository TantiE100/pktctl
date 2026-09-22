use std::time::Duration;

use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{
        devices::{describe, list},
        hosts::{HostConfigRequest, configure},
        network_file::{edit_saved_network, file_error},
        paths::device,
        physical::{MoveRequest, Snapshot, move_to_location},
        power::fast_forward,
    },
    packet_tracer::{
        PacketTracer, PtError, expect_bool, expect_integer, expect_number, expect_text,
    },
};

const SERVER: &str = "WirelessServerProcess";
const CLIENT: &str = "WirelessClientProcess";
const ASSOCIATION_POLLS: u32 = 15;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const WPA_KEY: std::ops::RangeInclusive<usize> = 8..=63;
/// Radio reach measured on Packet Tracer 9.0.1 in physical-workspace global units:
/// associations succeed at 110 and fail at 130 for every access point model tried.
const RADIO_RANGE: f64 = 120.0;
const COMFORTABLE_RANGE: f64 = 100.0;
const BESIDE_CLIENT: i64 = 30;
const WEP_64_HEX: usize = 10;
const WEP_128_HEX: usize = 26;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    #[default]
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
}

impl Security {
    fn wire(self, key: &str) -> Result<(i64, i64), PtError> {
        let key_len = key.chars().count();
        match self {
            Self::Open if key.is_empty() => Ok((0, 0)),
            Self::Open => Err(PtError::InvalidInput("an open network takes no key".into())),
            Self::Wep if !key.chars().all(|character| character.is_ascii_hexdigit()) => Err(
                PtError::InvalidInput("a WEP key is 10 or 26 hexadecimal digits".into()),
            ),
            Self::Wep if key_len == WEP_64_HEX => Ok((1, 1)),
            Self::Wep if key_len == WEP_128_HEX => Ok((1, 2)),
            Self::Wep => Err(PtError::InvalidInput(
                "a WEP key is 10 or 26 hexadecimal digits".into(),
            )),
            Self::WpaPsk | Self::Wpa2Psk if !WPA_KEY.contains(&key_len) => Err(
                PtError::InvalidInput("a WPA passphrase has 8 to 63 characters".into()),
            ),
            Self::WpaPsk => Ok((2, 3)),
            Self::Wpa2Psk => Ok((4, 4)),
        }
    }

    fn from_wire(authen: i64) -> Option<Self> {
        match authen {
            0 | 6 => Some(Self::Open),
            1 => Some(Self::Wep),
            2 => Some(Self::WpaPsk),
            4 => Some(Self::Wpa2Psk),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AccessPointRequest {
    /// Access point or wireless router name.
    pub device: String,
    pub ssid: String,
    #[serde(default)]
    pub security: Security,
    /// Passphrase for WPA (8-63 characters) or WEP key (10 or 26 hex digits).
    #[serde(default)]
    pub key: Option<String>,
    /// Advertise the SSID. Defaults to true.
    #[serde(default)]
    pub broadcast_ssid: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AccessPointConfig {
    pub device: String,
    pub ssid: String,
    pub security: Option<Security>,
    pub broadcast_ssid: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ConnectWirelessRequest {
    /// Client with a wireless card, for example a laptop.
    pub device: String,
    pub ssid: String,
    #[serde(default)]
    pub security: Security,
    #[serde(default)]
    pub key: Option<String>,
    /// Move the access point with this SSID next to the client first when it is out of radio
    /// range (Packet Tracer's radios reach about 120 units in the physical workspace).
    #[serde(default)]
    pub bring_access_point: bool,
    /// Static address. Omit for DHCP.
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub mask: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WirelessConnection {
    pub device: String,
    pub ssid: String,
    pub associated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_point: Option<String>,
    /// When the client did not associate: why, with the distance to each access point
    /// broadcasting the SSID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<String>,
    /// The access point that `bring_access_point` moved next to the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_access_point: Option<String>,
    /// Address of the wireless port; with DHCP, the lease if one arrived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// The file the network was saved to, edited and reopened from.
    pub file: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct StatusRequest {
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WirelessStatus {
    pub device: String,
    /// `access_point` or `client`.
    pub role: String,
    pub ssid: String,
    pub security: Option<Security>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast_ssid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_point: Option<String>,
}

fn process(name: &str, kind: &str) -> Call {
    device(name).method("getProcess", [Value::string(kind)])
}

async fn has_process<P: PacketTracer>(
    packet_tracer: &P,
    name: &str,
    kind: &str,
) -> Result<bool, PtError> {
    match packet_tracer
        .call(process(name, kind).method("getClassName", []))
        .await
    {
        Ok(_) => Ok(true),
        Err(PtError::NotFound(_) | PtError::Rejected(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

async fn apply_radio<P: PacketTracer>(
    packet_tracer: &P,
    radio: &Call,
    ssid: &str,
    (authen, encrypt): (i64, i64),
    key: &str,
    security: Security,
) -> Result<(), PtError> {
    let int = |value: i64| Value::Int(i32::try_from(value).unwrap_or_default());
    packet_tracer
        .call(radio.clone().method("setSsid", [Value::string(ssid)]))
        .await?;
    packet_tracer
        .call(radio.clone().method("setAuthenType", [int(authen)]))
        .await?;
    packet_tracer
        .call(radio.clone().method("setEncryptType", [int(encrypt)]))
        .await?;
    let key_process = if security == Security::Wep {
        "getWepProcess"
    } else {
        "getWpaProcess"
    };
    if security != Security::Open {
        packet_tracer
            .call(
                radio
                    .clone()
                    .method(key_process, [])
                    .method("setKey", [Value::string(key)]),
            )
            .await?;
    }
    Ok(())
}

async fn read_radio<P: PacketTracer>(
    packet_tracer: &P,
    radio: &Call,
) -> Result<(String, Option<Security>), PtError> {
    let (ssid, authen) = tokio::try_join!(
        packet_tracer.call(radio.clone().method("getSsid", [])),
        packet_tracer.call(radio.clone().method("getAuthenType", [])),
    )?;
    Ok((
        expect_text(&ssid, "SSID")?,
        Security::from_wire(expect_integer(&authen, "authentication type")?),
    ))
}

fn checked(
    ssid: &str,
    key: Option<&str>,
    security: Security,
) -> Result<((i64, i64), String), PtError> {
    if ssid.trim().is_empty() {
        return Err(PtError::InvalidInput("an SSID is required".into()));
    }
    let key = key.unwrap_or_default().to_owned();
    Ok((security.wire(&key)?, key))
}

pub async fn configure_access_point<P: PacketTracer>(
    packet_tracer: &P,
    request: &AccessPointRequest,
) -> Result<AccessPointConfig, PtError> {
    let name = request.device.trim();
    describe(packet_tracer, name).await?;
    if !has_process(packet_tracer, name, SERVER).await? {
        return Err(PtError::InvalidInput(format!(
            "`{name}` is not an access point or wireless router"
        )));
    }
    let (wire, key) = checked(&request.ssid, request.key.as_deref(), request.security)?;
    let radio = process(name, SERVER);
    apply_radio(
        packet_tracer,
        &radio,
        request.ssid.trim(),
        wire,
        &key,
        request.security,
    )
    .await?;
    packet_tracer
        .call(radio.clone().method(
            "setSsidBrdCastEnabled",
            [Value::Bool(request.broadcast_ssid.unwrap_or(true))],
        ))
        .await?;
    let (ssid, security) = read_radio(packet_tracer, &radio).await?;
    let broadcast = packet_tracer
        .call(radio.method("isSsidBrdCastEnabled", []))
        .await?;
    Ok(AccessPointConfig {
        device: name.to_owned(),
        ssid,
        security,
        broadcast_ssid: expect_bool(&broadcast, "SSID broadcast")?,
    })
}

pub async fn connect_wireless<P: PacketTracer>(
    packet_tracer: &P,
    request: &ConnectWirelessRequest,
) -> Result<WirelessConnection, PtError> {
    let name = request.device.trim().to_owned();
    describe(packet_tracer, &name).await?;
    if !has_process(packet_tracer, &name, CLIENT).await? || !has_radio(packet_tracer, &name).await?
    {
        return Err(PtError::InvalidInput(format!(
            "`{name}` has no wireless card; install one first, for example add_module with \
             PT-LAPTOP-NM-1W on a laptop or PT-HOST-NM-1W on a PC"
        )));
    }
    let (wire, key) = checked(&request.ssid, request.key.as_deref(), request.security)?;
    let text = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let dhcp = request.ip.as_deref().is_none_or(|ip| ip.trim().is_empty());
    if !dhcp
        && request
            .mask
            .as_deref()
            .is_none_or(|mask| mask.trim().is_empty())
    {
        return Err(PtError::InvalidInput(
            "a static address needs a mask".into(),
        ));
    }
    let ssid = request.ssid.trim().to_owned();
    let profile = pktfile::ClientProfile {
        ssid: ssid.clone(),
        authen_type: wire.0,
        encrypt_type: wire.1,
        key: key.clone(),
        dhcp,
        ip: text(&request.ip),
        mask: text(&request.mask),
        gateway: text(&request.gateway),
        dns: text(&request.dns),
    };

    let moved_access_point = if request.bring_access_point {
        bring_access_point(packet_tracer, &name, &ssid).await?
    } else {
        None
    };
    apply_radio(
        packet_tracer,
        &process(&name, CLIENT),
        &ssid,
        wire,
        &key,
        request.security,
    )
    .await?;
    let file = edit_saved_network(packet_tracer, |xml| {
        pktfile::set_client_profile(xml, &name, &profile).map_err(|error| file_error(&error))
    })
    .await?;

    let access_point = wait_for_association(packet_tracer, &name).await?;
    let addressing = HostConfigRequest {
        device: name.clone(),
        port: Some(radio_port(packet_tracer, &name).await?),
        dhcp,
        ip: request.ip.clone().filter(|_| !dhcp),
        mask: request.mask.clone().filter(|_| !dhcp),
        gateway: request
            .gateway
            .clone()
            .filter(|gateway| !dhcp && !gateway.trim().is_empty()),
        dns: request.dns.clone().filter(|dns| !dns.trim().is_empty()),
    };
    let addressed = configure(packet_tracer, &addressing).await?;
    let diagnosis = if access_point.is_some() {
        None
    } else {
        Some(diagnose(packet_tracer, &name, &ssid).await?)
    };
    Ok(WirelessConnection {
        device: name,
        ssid,
        associated: access_point.is_some(),
        access_point,
        diagnosis,
        moved_access_point,
        ip: addressed.ip,
        file,
    })
}

async fn global_position<P: PacketTracer>(
    packet_tracer: &P,
    name: &str,
) -> Result<(f64, f64), PtError> {
    let physical = device(name).method("getPhysicalObject", []);
    let (x, y) = tokio::try_join!(
        packet_tracer.call(physical.clone().method("getGlobalX", [])),
        packet_tracer.call(physical.method("getGlobalY", [])),
    )?;
    Ok((
        expect_number(&x, "global x")?,
        expect_number(&y, "global y")?,
    ))
}

async fn access_points_for<P: PacketTracer>(
    packet_tracer: &P,
    client: &str,
    ssid: &str,
) -> Result<Vec<(String, f64)>, PtError> {
    let here = global_position(packet_tracer, client).await?;
    let mut found = Vec::new();
    for candidate in list(packet_tracer).await?.devices {
        if candidate.name == client || !has_process(packet_tracer, &candidate.name, SERVER).await? {
            continue;
        }
        let (candidate_ssid, _) =
            read_radio(packet_tracer, &process(&candidate.name, SERVER)).await?;
        if candidate_ssid != ssid {
            continue;
        }
        let there = global_position(packet_tracer, &candidate.name).await?;
        found.push((candidate.name, (here.0 - there.0).hypot(here.1 - there.1)));
    }
    found.sort_by(|left, right| left.1.total_cmp(&right.1));
    Ok(found)
}

async fn bring_access_point<P: PacketTracer>(
    packet_tracer: &P,
    client: &str,
    ssid: &str,
) -> Result<Option<String>, PtError> {
    let candidates = access_points_for(packet_tracer, client, ssid).await?;
    let Some((nearest, distance)) = candidates.into_iter().next() else {
        return Err(PtError::InvalidInput(format!(
            "no access point broadcasts SSID `{ssid}`; configure one with configure_access_point"
        )));
    };
    if distance <= COMFORTABLE_RANGE {
        return Ok(None);
    }
    let snapshot = Snapshot::read(packet_tracer).await?;
    let spot = snapshot.device(client)?;
    let parent = spot.parent.clone().unwrap_or_default();
    move_to_location(
        packet_tracer,
        &MoveRequest {
            device: Some(nearest.clone()),
            location: None,
            into: parent,
            x: i32::try_from(spot.x + BESIDE_CLIENT).ok(),
            y: i32::try_from(spot.y).ok(),
        },
    )
    .await?;
    Ok(Some(nearest))
}

async fn diagnose<P: PacketTracer>(
    packet_tracer: &P,
    client: &str,
    ssid: &str,
) -> Result<String, PtError> {
    let candidates = access_points_for(packet_tracer, client, ssid).await?;
    if candidates.is_empty() {
        return Ok(format!(
            "no access point broadcasts SSID `{ssid}`; configure one with configure_access_point"
        ));
    }
    let listed: Vec<String> = candidates
        .iter()
        .map(|(name, distance)| format!("{name} is {distance:.0} units away"))
        .collect();
    let nearest = candidates[0].1;
    let advice = if nearest > RADIO_RANGE {
        "out of radio range (about 120 units): move it closer with move_to_location, or call \
         connect_wireless again with bring_access_point: true"
    } else {
        "in range, so check the security and key match the access point"
    };
    Ok(format!("{}; {advice}", listed.join(", ")))
}

async fn has_radio<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<bool, PtError> {
    Ok(radio_port(packet_tracer, name).await.is_ok())
}

async fn radio_port<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<String, PtError> {
    let count = packet_tracer
        .call(device(name).method("getPortCount", []))
        .await?;
    for index in 0..expect_integer(&count, "port count")? {
        let port = device(name).method(
            "getPortAt",
            [Value::Int(i32::try_from(index).unwrap_or_default())],
        );
        let wireless = packet_tracer
            .call(port.clone().method("isWirelessPort", []))
            .await?;
        if expect_bool(&wireless, "isWirelessPort")? {
            let port_name = packet_tracer.call(port.method("getName", [])).await?;
            return expect_text(&port_name, "port name");
        }
    }
    Err(PtError::NotFound(format!("wireless port on `{name}`")))
}

async fn wait_for_association<P: PacketTracer>(
    packet_tracer: &P,
    name: &str,
) -> Result<Option<String>, PtError> {
    for attempt in 0..ASSOCIATION_POLLS {
        if let Some(access_point) = associated_with(packet_tracer, name).await? {
            return Ok(Some(access_point));
        }
        if attempt + 1 < ASSOCIATION_POLLS {
            fast_forward(packet_tracer).await?;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
    Ok(None)
}

async fn associated_with<P: PacketTracer>(
    packet_tracer: &P,
    name: &str,
) -> Result<Option<String>, PtError> {
    let mac = packet_tracer
        .call(process(name, CLIENT).method("getCurrentApMac", []))
        .await?;
    let mac = expect_text(&mac, "access point MAC")?;
    if mac.trim().is_empty() {
        return Ok(None);
    }
    for candidate in list(packet_tracer).await?.devices {
        if candidate.name == name || !has_process(packet_tracer, &candidate.name, SERVER).await? {
            continue;
        }
        let count = packet_tracer
            .call(device(&candidate.name).method("getPortCount", []))
            .await?;
        for index in 0..expect_integer(&count, "port count")? {
            let port = device(&candidate.name).method(
                "getPortAt",
                [Value::Int(i32::try_from(index).unwrap_or_default())],
            );
            let wireless = packet_tracer
                .call(port.clone().method("isWirelessPort", []))
                .await?;
            if !expect_bool(&wireless, "isWirelessPort")? {
                continue;
            }
            let port_mac = packet_tracer.call(port.method("getMacAddress", [])).await?;
            if expect_text(&port_mac, "port MAC")?.eq_ignore_ascii_case(mac.trim()) {
                return Ok(Some(candidate.name));
            }
        }
    }
    Ok(Some(mac))
}

pub async fn status<P: PacketTracer>(
    packet_tracer: &P,
    request: &StatusRequest,
) -> Result<WirelessStatus, PtError> {
    let name = request.device.trim();
    describe(packet_tracer, name).await?;
    if has_process(packet_tracer, name, SERVER).await? {
        let radio = process(name, SERVER);
        let (ssid, security) = read_radio(packet_tracer, &radio).await?;
        let broadcast = packet_tracer
            .call(radio.method("isSsidBrdCastEnabled", []))
            .await?;
        return Ok(WirelessStatus {
            device: name.to_owned(),
            role: "access_point".into(),
            ssid,
            security,
            broadcast_ssid: Some(expect_bool(&broadcast, "SSID broadcast")?),
            access_point: None,
        });
    }
    if has_process(packet_tracer, name, CLIENT).await? && has_radio(packet_tracer, name).await? {
        let (ssid, security) = read_radio(packet_tracer, &process(name, CLIENT)).await?;
        return Ok(WirelessStatus {
            device: name.to_owned(),
            role: "client".into(),
            ssid,
            security,
            broadcast_ssid: None,
            access_point: associated_with(packet_tracer, name).await?,
        });
    }
    Err(PtError::InvalidInput(format!(
        "`{name}` has no wireless radio"
    )))
}
