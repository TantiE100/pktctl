use ptmp::{Step, TypeCode, Value};

use super::{
    Endpoint, Link, State,
    remote::{Remote, check_args, no_args},
};

const SERVER: &str = "WirelessServerProcess";
const CLIENT: &str = "WirelessClientProcess";
const WIRELESS_LINK: i32 = 8109;
const OPEN: i64 = 0;
pub(super) const CLIENT_PORT: &str = "Wireless0";
pub(super) const AP_PORT: &str = "Port 1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Radio {
    pub(super) ssid: String,
    pub(super) authen: i64,
    pub(super) encrypt: i64,
    pub(super) key: String,
    pub(super) broadcast: bool,
}

impl Default for Radio {
    fn default() -> Self {
        Self {
            ssid: "Default".into(),
            authen: OPEN,
            encrypt: OPEN,
            key: String::new(),
            broadcast: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Client {
    pub(super) common: Radio,
    pub(super) profile: pktfile::ClientProfile,
    pub(super) associated: Option<String>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            common: Radio::default(),
            profile: pktfile::ClientProfile {
                ssid: "Default".into(),
                dhcp: true,
                ..pktfile::ClientProfile::default()
            },
            associated: None,
        }
    }
}

pub(super) fn process(
    state: &mut State,
    index: usize,
    name: &str,
    steps: &[Step],
) -> Result<Value, Remote> {
    let device = &state.devices[index];
    let class = match (name, device.access_radio.is_some(), device.client.is_some()) {
        ("WirelessServerProcess" | "WirelessServer", true, _) => SERVER,
        ("WirelessClientProcess" | "WirelessClient", _, true) => CLIENT,
        _ => return Err(Remote::missing("Process")),
    };
    if matches!(steps, [step] if step.method == "getClassName") {
        return Ok(Value::string(class));
    }
    if class == CLIENT && matches!(steps, [step] if step.method == "getCurrentApMac") {
        let mac = state.devices[index]
            .client
            .as_ref()
            .and_then(|client| client.associated.clone())
            .and_then(|ap| state.devices.iter().position(|device| device.name == ap))
            .map(radio_mac)
            .unwrap_or_default();
        return Ok(Value::Mac(mac));
    }
    let device = &mut state.devices[index];
    let radio = match class {
        SERVER => device.access_radio.as_mut(),
        _ => device.client.as_mut().map(|client| &mut client.common),
    }
    .ok_or_else(|| Remote::missing("Process"))?;
    radio_call(radio, class, steps)
}

fn radio_call(radio: &mut Radio, class: &str, steps: &[Step]) -> Result<Value, Remote> {
    match steps {
        [key_process, step]
            if key_process.method == "getWpaProcess" || key_process.method == "getWepProcess" =>
        {
            match step.method.as_str() {
                "setKey" => {
                    check_args(step, "WPAProcess", &[TypeCode::String])?;
                    step.args[0]
                        .as_str()
                        .unwrap_or_default()
                        .clone_into(&mut radio.key);
                    Ok(Value::Void)
                }
                "getKey" => no_args(step, "WPAProcess").map(|()| Value::string(&radio.key)),
                other => Err(Remote::unknown_method("WPAProcess", other)),
            }
        }
        [step] => match step.method.as_str() {
            "setSsid" => {
                check_args(step, class, &[TypeCode::String])?;
                step.args[0]
                    .as_str()
                    .unwrap_or_default()
                    .clone_into(&mut radio.ssid);
                Ok(Value::Void)
            }
            "getSsid" => no_args(step, class).map(|()| Value::string(&radio.ssid)),
            "setAuthenType" => {
                check_args(step, class, &[TypeCode::Int])?;
                radio.authen = step.args[0].as_i64().unwrap_or_default();
                Ok(Value::Void)
            }
            "getAuthenType" => no_args(step, class).map(|()| Value::Int(narrow(radio.authen))),
            "setEncryptType" => {
                check_args(step, class, &[TypeCode::Int])?;
                radio.encrypt = step.args[0].as_i64().unwrap_or_default();
                Ok(Value::Void)
            }
            "getEncryptType" => no_args(step, class).map(|()| Value::Int(narrow(radio.encrypt))),
            "setSsidBrdCastEnabled" if class == SERVER => {
                check_args(step, class, &[TypeCode::Bool])?;
                radio.broadcast = step.args[0].as_bool().unwrap_or_default();
                Ok(Value::Void)
            }
            "isSsidBrdCastEnabled" if class == SERVER => {
                no_args(step, class).map(|()| Value::Bool(radio.broadcast))
            }
            other => Err(Remote::unknown_method(class, other)),
        },
        _ => Err(Remote::unknown_method(class, "")),
    }
}

fn narrow(value: i64) -> i32 {
    i32::try_from(value).unwrap_or_default()
}

pub(super) fn radio_mac(index: usize) -> String {
    format!("00E0.F7{:02X}.{:04X}", index % 256, index)
}

/// Associates every client whose current profile matches an access point, as Packet
/// Tracer does when a file is opened or a radio appears.
pub(super) fn associate(state: &mut State) {
    let access_points: Vec<(String, Radio)> = state
        .devices
        .iter()
        .filter_map(|device| {
            device
                .access_radio
                .clone()
                .map(|radio| (device.name.clone(), radio))
        })
        .collect();
    let mut links = Vec::new();
    for device in &mut state.devices {
        let Some(client) = device.client.as_mut() else {
            continue;
        };
        let profile = &client.profile;
        client.associated = access_points
            .iter()
            .find(|(_, radio)| {
                radio.ssid == profile.ssid
                    && radio.authen == profile.authen_type
                    && radio.encrypt == profile.encrypt_type
                    && (radio.authen == OPEN || radio.key == profile.key)
            })
            .map(|(name, _)| name.clone());
        if let Some(ap) = &client.associated {
            links.push(Link {
                ends: [
                    Endpoint {
                        device: device.name.clone(),
                        port: CLIENT_PORT.into(),
                    },
                    Endpoint {
                        device: ap.clone(),
                        port: AP_PORT.into(),
                    },
                ],
                cable: WIRELESS_LINK,
            });
        }
    }
    state.links.retain(|link| link.cable != WIRELESS_LINK);
    state.links.extend(links);
}

pub(super) fn profile_xml(profile: &pktfile::ClientProfile) -> String {
    let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        "<WIRELESS_CLIENT><CURRENT_PROFILE><WIRELESS_PROFILE><NAME>{ssid}</NAME><SSID>{ssid}</SSID>\
         <NETWORK_TYPE>7</NETWORK_TYPE><AUTHEN_TYPE>{}</AUTHEN_TYPE><ENCRYPT_TYPE>{}</ENCRYPT_TYPE>\
         <WEP_KEY>{}</WEP_KEY><DHCP_ENABLED>{}</DHCP_ENABLED><IP_ADDRESS>{}</IP_ADDRESS>\
         <SUBNET_MASK>{}</SUBNET_MASK><DEFAULT_GATEWAY>{}</DEFAULT_GATEWAY><DNS>{}</DNS>\
         </WIRELESS_PROFILE></CURRENT_PROFILE></WIRELESS_CLIENT>",
        profile.authen_type,
        profile.encrypt_type,
        escape(&profile.key),
        u8::from(profile.dhcp),
        escape(&profile.ip),
        escape(&profile.mask),
        escape(&profile.gateway),
        escape(&profile.dns),
        ssid = escape(&profile.ssid),
    )
}
