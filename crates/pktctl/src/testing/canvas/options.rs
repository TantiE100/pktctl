use std::collections::BTreeMap;

use ptmp::{Step, Value};

use super::remote::{Remote, no_args};

const CLASS: &str = "Options";
const PAIRS: &[(&str, &str)] = &[
    ("isPortShown", "setIsPortShown"),
    ("isPortNotShownOnMouseOver", "setPortNotShownOnMouseOver"),
    ("isLinkLightsShown", "setIsLinkLightShown"),
    ("isHideDevLabel", "setHideDevLabel"),
    ("isHideDevModelLabel", "setHideDevModelLabel"),
    ("isHideQoSStamp", "setHideQoSStamp"),
    ("isAutoCablingDisabled", "setDisableAutoCabling"),
    ("isEnableCableLengthEffects", "setEnableCableLengthEffects"),
    ("isAnimation", "setAnimation"),
    ("isSound", "setSound"),
    ("isTelephonySound", "setTelephonySound"),
    ("isLoggingEnabled", "setIsLoggingEnabled"),
    ("isUsingMetric", "setUseMetric"),
    (
        "isExternalNetworkAccessEnabled",
        "setEnableExternalNetworkAccess",
    ),
    ("isUseCliDefaultTab", "setUseCliDefaultTab"),
    ("isDevTaskbarShown", "setShowDevTaskbar"),
    ("isCableInfoPopup", "setCableInfoPopup"),
    ("isPhysicalTabHidden", "setPhysicalTabHidden"),
    ("isConfigTabHidden", "setConfigTabHidden"),
    ("isCliTabHidden", "setCliTabHidden"),
    ("isDesktopTabHidden", "setDesktopTabHidden"),
    ("isGuiTabHidden", "setGuiTabHidden"),
    ("isChallenge_PDUInfo", "setIsChallenge_PDUInfo"),
    ("isAccessible", "setAccessible"),
    ("isDockFirst", "setIsDockFirst"),
    ("isMainToolbarShown", "setMainToolbarShown"),
    ("isSecondaryToolbarShown", "setSecondaryToolbarShown"),
    ("isBottomToolbarShown", "setBottomToolbarShown"),
];
const TWO_ARGUMENT_SETTERS: &[&str] =
    &["setHideDevLabel", "setHideDevModelLabel", "setHideQoSStamp"];

pub(super) fn handle(
    values: &mut BTreeMap<&'static str, bool>,
    steps: &[Step],
) -> Result<Value, Remote> {
    let [step] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    if let Some((getter, _)) = PAIRS.iter().find(|(getter, _)| *getter == step.method) {
        no_args(step, CLASS)?;
        return Ok(Value::Bool(
            values
                .get(getter)
                .copied()
                .unwrap_or(*getter == "isPortShown"),
        ));
    }
    if let Some((getter, setter)) = PAIRS.iter().find(|(_, setter)| *setter == step.method) {
        let arity = if TWO_ARGUMENT_SETTERS.contains(setter) {
            2
        } else {
            1
        };
        let flags: Vec<bool> = step.args.iter().filter_map(Value::as_bool).collect();
        if flags.len() != arity || step.args.len() != arity {
            return Err(Remote {
                class: CLASS.into(),
                message: format!("Invalid arguments for IPC call \"{setter}\""),
            });
        }
        values.insert(getter, flags[0]);
        return Ok(Value::Void);
    }
    Err(Remote::unknown_method(CLASS, &step.method))
}
