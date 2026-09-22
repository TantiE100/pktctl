use std::collections::BTreeMap;

use ptmp::{Call, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    packet_tracer::{PacketTracer, PtError, expect_bool},
    server::PktctlServer,
};

struct Preference {
    name: &'static str,
    getter: &'static str,
    setter: &'static str,
    workspace_flag: bool,
}

const fn pref(name: &'static str, getter: &'static str, setter: &'static str) -> Preference {
    Preference {
        name,
        getter,
        setter,
        workspace_flag: false,
    }
}

const fn workspace_pref(
    name: &'static str,
    getter: &'static str,
    setter: &'static str,
) -> Preference {
    Preference {
        name,
        getter,
        setter,
        workspace_flag: true,
    }
}

const PREFERENCES: &[Preference] = &[
    pref("show_port_labels", "isPortShown", "setIsPortShown"),
    pref(
        "hide_port_labels_on_hover",
        "isPortNotShownOnMouseOver",
        "setPortNotShownOnMouseOver",
    ),
    pref(
        "show_link_lights",
        "isLinkLightsShown",
        "setIsLinkLightShown",
    ),
    workspace_pref("hide_device_names", "isHideDevLabel", "setHideDevLabel"),
    workspace_pref(
        "hide_device_models",
        "isHideDevModelLabel",
        "setHideDevModelLabel",
    ),
    workspace_pref("hide_qos_stamps", "isHideQoSStamp", "setHideQoSStamp"),
    pref(
        "disable_auto_cabling",
        "isAutoCablingDisabled",
        "setDisableAutoCabling",
    ),
    pref(
        "cable_length_effects",
        "isEnableCableLengthEffects",
        "setEnableCableLengthEffects",
    ),
    pref("animation", "isAnimation", "setAnimation"),
    pref("sound", "isSound", "setSound"),
    pref("telephony_sound", "isTelephonySound", "setTelephonySound"),
    pref("logging", "isLoggingEnabled", "setIsLoggingEnabled"),
    pref("metric_units", "isUsingMetric", "setUseMetric"),
    pref(
        "external_network_access",
        "isExternalNetworkAccessEnabled",
        "setEnableExternalNetworkAccess",
    ),
    pref(
        "cli_tab_by_default",
        "isUseCliDefaultTab",
        "setUseCliDefaultTab",
    ),
    pref(
        "show_device_taskbar",
        "isDevTaskbarShown",
        "setShowDevTaskbar",
    ),
    pref("cable_info_popup", "isCableInfoPopup", "setCableInfoPopup"),
    pref(
        "hide_physical_tab",
        "isPhysicalTabHidden",
        "setPhysicalTabHidden",
    ),
    pref("hide_config_tab", "isConfigTabHidden", "setConfigTabHidden"),
    pref("hide_cli_tab", "isCliTabHidden", "setCliTabHidden"),
    pref(
        "hide_desktop_tab",
        "isDesktopTabHidden",
        "setDesktopTabHidden",
    ),
    pref("hide_gui_tab", "isGuiTabHidden", "setGuiTabHidden"),
    pref(
        "challenge_pdu_info",
        "isChallenge_PDUInfo",
        "setIsChallenge_PDUInfo",
    ),
    pref("accessibility", "isAccessible", "setAccessible"),
    pref("dock_first", "isDockFirst", "setIsDockFirst"),
    pref(
        "show_main_toolbar",
        "isMainToolbarShown",
        "setMainToolbarShown",
    ),
    pref(
        "show_secondary_toolbar",
        "isSecondaryToolbarShown",
        "setSecondaryToolbarShown",
    ),
    pref(
        "show_bottom_toolbar",
        "isBottomToolbarShown",
        "setBottomToolbarShown",
    ),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetPreferencesRequest {
    /// Preferences to change, for example `{ "show_port_labels": true, "disable_auto_cabling": true }`.
    /// `get_preferences` lists every name.
    pub values: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Preferences {
    pub values: BTreeMap<String, bool>,
}

fn options() -> Call {
    Call::root("options")
}

pub async fn get_preferences<P: PacketTracer>(packet_tracer: &P) -> Result<Preferences, PtError> {
    let replies = futures::future::try_join_all(
        PREFERENCES
            .iter()
            .map(|preference| packet_tracer.call(options().method(preference.getter, []))),
    )
    .await?;
    let mut values = BTreeMap::new();
    for (preference, reply) in PREFERENCES.iter().zip(replies) {
        values.insert(
            preference.name.to_owned(),
            expect_bool(&reply, preference.name)?,
        );
    }
    Ok(Preferences { values })
}

pub async fn set_preferences<P: PacketTracer>(
    packet_tracer: &P,
    request: &SetPreferencesRequest,
) -> Result<Preferences, PtError> {
    let mut changes = Vec::new();
    for (name, value) in &request.values {
        let preference = PREFERENCES
            .iter()
            .find(|preference| preference.name == name.trim())
            .ok_or_else(|| {
                let names: Vec<&str> = PREFERENCES
                    .iter()
                    .map(|preference| preference.name)
                    .collect();
                PtError::InvalidInput(format!(
                    "unknown preference `{name}`; known: {}",
                    names.join(", ")
                ))
            })?;
        changes.push((preference, *value));
    }
    for (preference, value) in changes {
        let mut args = vec![Value::Bool(value)];
        if preference.workspace_flag {
            args.push(Value::Bool(true));
        }
        packet_tracer
            .call(options().method(preference.setter, args))
            .await?;
    }
    get_preferences(packet_tracer).await
}

#[tool_router(router = preferences_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "get_preferences",
        description = "Read Packet Tracer's preferences: port labels, link lights, device \
                       labels, auto cabling, animation, sound, device dialog tabs, toolbars and \
                       more.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_preferences_tool(&self) -> Result<Json<Preferences>, String> {
        get_preferences(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "set_preferences",
        description = "Change Packet Tracer preferences by name, for example \
                       `{ \"values\": { \"show_port_labels\": true } }`. Returns every \
                       preference afterwards.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn set_preferences_tool(
        &self,
        Parameters(request): Parameters<SetPreferencesRequest>,
    ) -> Result<Json<Preferences>, String> {
        set_preferences(self.packet_tracer(), &request)
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
        packet_tracer::{api::ApiIndex, scripted::ScriptedPacketTracer},
        testing::Canvas,
    };

    #[test]
    fn every_preference_maps_to_real_options_methods() {
        let api = ApiIndex::get();
        for preference in PREFERENCES {
            let getter = api.methods_named("Options", preference.getter);
            assert_eq!(getter.len(), 1, "{}", preference.getter);
            assert_eq!(getter[0].1.returns, "bool");
            let setter = api.methods_named("Options", preference.setter);
            assert_eq!(setter.len(), 1, "{}", preference.setter);
            let arity = if preference.workspace_flag { 2 } else { 1 };
            assert_eq!(
                setter[0].1.params,
                vec!["bool"; arity],
                "{}",
                preference.setter
            );
        }
    }

    #[tokio::test]
    async fn sets_preferences_and_reads_them_back() {
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::new(Canvas::new()));
        let before = get_preferences(&packet_tracer).await.unwrap();
        assert_eq!(before.values.len(), PREFERENCES.len());
        let after = set_preferences(
            &packet_tracer,
            &SetPreferencesRequest {
                values: BTreeMap::from([
                    (
                        "show_port_labels".to_owned(),
                        !before.values["show_port_labels"],
                    ),
                    ("hide_device_names".to_owned(), true),
                ]),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            after.values["show_port_labels"],
            !before.values["show_port_labels"]
        );
        assert!(after.values["hide_device_names"]);

        let unknown = set_preferences(
            &packet_tracer,
            &SetPreferencesRequest {
                values: BTreeMap::from([("dark_mode".to_owned(), true)]),
            },
        )
        .await
        .unwrap_err();
        assert!(unknown.to_string().contains("show_port_labels"));
    }
}
