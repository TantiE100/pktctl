use crate::{
    PktError,
    elements::{Element, elements, splice},
};

/// What a wireless client connects with: the fields of its current profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientProfile {
    pub ssid: String,
    pub authen_type: i64,
    pub encrypt_type: i64,
    pub key: String,
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
}

const PROFILE_PATH: &[&str] = &[
    "ENGINE",
    "WIRELESS_CLIENT",
    "CURRENT_PROFILE",
    "WIRELESS_PROFILE",
];

/// Rewrites the current wireless profile of `device`, the one Packet Tracer uses to
/// associate when the file is opened.
pub fn set_client_profile(
    xml: &str,
    device: &str,
    profile: &ClientProfile,
) -> Result<String, PktError> {
    let all = elements(xml)?;
    let device_element = find_device(xml, &all, device)?;
    let depth = device_element.path.len();
    let current = all
        .iter()
        .find(|element| {
            device_element.contains(element)
                && element.path.len() == depth + PROFILE_PATH.len()
                && element.path[depth..]
                    .iter()
                    .map(String::as_str)
                    .eq(PROFILE_PATH.iter().copied())
        })
        .ok_or_else(|| PktError::NodeNotFound(format!("wireless client profile of {device}")))?;
    let fields = [
        ("NAME", escape(&profile.ssid)),
        ("SSID", escape(&profile.ssid)),
        ("AUTHEN_TYPE", profile.authen_type.to_string()),
        ("ENCRYPT_TYPE", profile.encrypt_type.to_string()),
        ("WEP_KEY", escape(&profile.key)),
        ("DHCP_ENABLED", u8::from(profile.dhcp).to_string()),
        ("IP_ADDRESS", escape(&profile.ip)),
        ("SUBNET_MASK", escape(&profile.mask)),
        ("DEFAULT_GATEWAY", escape(&profile.gateway)),
        ("DNS", escape(&profile.dns)),
    ];
    let mut edits = Vec::new();
    for (name, value) in fields {
        let field = all
            .iter()
            .find(|element| element.is_child_of(current) && element.name() == name)
            .ok_or_else(|| PktError::NodeNotFound(format!("{name} in the profile of {device}")))?;
        let replacement = format!("<{name}>{value}</{name}>");
        edits.push((field.outer.clone(), replacement));
    }
    Ok(splice(xml, edits))
}

/// Reads the current wireless profile of `device`.
pub fn client_profile(xml: &str, device: &str) -> Result<ClientProfile, PktError> {
    let all = elements(xml)?;
    let current = current_profile(xml, &all, device)?;
    let field = |name: &str| {
        all.iter()
            .find(|element| element.is_child_of(current) && element.name() == name)
            .map(|element| unescape(element.text(xml)))
            .unwrap_or_default()
    };
    Ok(ClientProfile {
        ssid: field("SSID"),
        authen_type: field("AUTHEN_TYPE").trim().parse().unwrap_or_default(),
        encrypt_type: field("ENCRYPT_TYPE").trim().parse().unwrap_or_default(),
        key: field("WEP_KEY"),
        dhcp: field("DHCP_ENABLED").trim() == "1",
        ip: field("IP_ADDRESS"),
        mask: field("SUBNET_MASK"),
        gateway: field("DEFAULT_GATEWAY"),
        dns: field("DNS"),
    })
}

fn current_profile<'a>(
    xml: &str,
    all: &'a [Element],
    device: &str,
) -> Result<&'a Element, PktError> {
    let device_element = find_device(xml, all, device)?;
    let depth = device_element.path.len();
    all.iter()
        .find(|element| {
            device_element.contains(element)
                && element.path.len() == depth + PROFILE_PATH.len()
                && element.path[depth..]
                    .iter()
                    .map(String::as_str)
                    .eq(PROFILE_PATH.iter().copied())
        })
        .ok_or_else(|| PktError::NodeNotFound(format!("wireless client profile of {device}")))
}

fn find_device<'a>(xml: &str, all: &'a [Element], device: &str) -> Result<&'a Element, PktError> {
    all.iter()
        .filter(|element| {
            element.name() == "DEVICE" && element.path.iter().any(|part| part == "DEVICES")
        })
        .find(|candidate| {
            all.iter().any(|element| {
                candidate.contains(element)
                    && element.path.len() == candidate.path.len() + 2
                    && element.path[candidate.path.len()] == "ENGINE"
                    && element.name() == "NAME"
                    && unescape(element.text(xml)) == device
            })
        })
        .ok_or_else(|| PktError::NodeNotFound(format!("device {device}")))
}

fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

fn unescape(text: &str) -> String {
    quick_xml::escape::unescape(text).map_or_else(|_| text.to_owned(), std::borrow::Cow::into_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<PACKETTRACER5><NETWORK><DEVICES>
<DEVICE><ENGINE><NAME translate="true">AP</NAME></ENGINE></DEVICE>
<DEVICE><ENGINE><NAME translate="true">LT</NAME>
<WIRELESS_CLIENT><WIRELESS_COMMON><SSID>Default</SSID></WIRELESS_COMMON>
<CURRENT_PROFILE><WIRELESS_PROFILE><NAME>Default</NAME><SSID>Default</SSID><NETWORK_TYPE>7</NETWORK_TYPE>
<AUTHEN_TYPE>0</AUTHEN_TYPE><ENCRYPT_TYPE>0</ENCRYPT_TYPE><WEP_KEY></WEP_KEY><DHCP_ENABLED>1</DHCP_ENABLED>
<IP_ADDRESS/><SUBNET_MASK/><DEFAULT_GATEWAY/><DNS/><VLAN>1</VLAN></WIRELESS_PROFILE></CURRENT_PROFILE>
<OTHER><WIRELESS_CLIENT><CURRENT_PROFILE><WIRELESS_PROFILE><SSID>ptcellular</SSID></WIRELESS_PROFILE></CURRENT_PROFILE></WIRELESS_CLIENT></OTHER>
</WIRELESS_CLIENT></ENGINE></DEVICE>
</DEVICES></NETWORK></PACKETTRACER5>"#;

    #[test]
    fn rewrites_only_the_wifi_profile_of_the_device() {
        let profile = ClientProfile {
            ssid: "GAMC & Co".into(),
            authen_type: 4,
            encrypt_type: 4,
            key: "clave1234".into(),
            dhcp: false,
            ip: "192.168.50.20".into(),
            mask: "255.255.255.0".into(),
            gateway: "192.168.50.1".into(),
            dns: String::new(),
        };
        let edited = set_client_profile(XML, "LT", &profile).unwrap();
        assert!(edited.contains(
            "<NAME>GAMC &amp; Co</NAME><SSID>GAMC &amp; Co</SSID><NETWORK_TYPE>7</NETWORK_TYPE>"
        ));
        assert!(edited.contains("<AUTHEN_TYPE>4</AUTHEN_TYPE><ENCRYPT_TYPE>4</ENCRYPT_TYPE><WEP_KEY>clave1234</WEP_KEY><DHCP_ENABLED>0</DHCP_ENABLED>"));
        assert!(edited.contains("<IP_ADDRESS>192.168.50.20</IP_ADDRESS>"));
        assert!(edited.contains("<DNS></DNS><VLAN>1</VLAN>"));
        assert!(edited.contains("<SSID>ptcellular</SSID>"));
        assert!(edited.contains("<WIRELESS_COMMON><SSID>Default</SSID>"));
        assert_eq!(client_profile(&edited, "LT").unwrap(), profile);
    }

    #[test]
    fn explains_devices_without_wifi() {
        assert_eq!(
            set_client_profile(XML, "AP", &ClientProfile::default()),
            Err(PktError::NodeNotFound(
                "wireless client profile of AP".into()
            ))
        );
        assert_eq!(
            set_client_profile(XML, "ZZ", &ClientProfile::default()),
            Err(PktError::NodeNotFound("device ZZ".into()))
        );
    }
}
