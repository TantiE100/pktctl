// Generated from com.cisco.pt.ipc.enums.{DeviceType,ModuleType} in the official Java framework.
pub(crate) const DEVICE_KINDS: &[(i64, &str)] = &[
    (0, "router"),
    (1, "switch"),
    (2, "cloud"),
    (3, "bridge"),
    (4, "hub"),
    (5, "repeater"),
    (6, "co_axial_splitter"),
    (7, "access_point"),
    (8, "pc"),
    (9, "server"),
    (10, "printer"),
    (11, "wireless_router"),
    (12, "ip_phone"),
    (13, "dsl_modem"),
    (14, "cable_modem"),
    (15, "remote_network"),
    (16, "multi_layer_switch"),
    (17, "switch3650"),
    (18, "laptop"),
    (19, "tablet_pc"),
    (20, "pda"),
    (21, "wireless_end_device"),
    (22, "wired_end_device"),
    (23, "tv"),
    (24, "home_voip"),
    (25, "analog_phone"),
    (26, "multi_user"),
    (27, "asa"),
    (28, "io_e"),
    (29, "home_gateway"),
    (30, "wireless_router_new_generation"),
    (31, "cell_tower"),
    (32, "central_office_server"),
    (33, "cisco_access_point"),
    (34, "embedded_cisco_access_point"),
    (35, "sniffer"),
    (36, "mcu"),
    (37, "sbc"),
    (38, "thing"),
    (39, "mcucomponent"),
    (40, "embedded_server"),
    (41, "wireless_lan_controller"),
    (42, "cluster"),
    (43, "geo_icon"),
    (44, "light_weight_access_point"),
    (45, "power_distribution_device"),
    (46, "patch_panel"),
    (47, "wall_mount"),
    (48, "security_appliance"),
    (49, "meraki_server"),
    (50, "network_controller"),
    (51, "plc"),
    (54, "cyber_observer"),
    (55, "data_historian"),
];

const IOS_KINDS: &[&str] = &[
    "router",
    "switch",
    "multi_layer_switch",
    "switch3650",
    "asa",
    "security_appliance",
];

pub(crate) const MODULE_KINDS: &[(i64, &str)] = &[
    (0, "line_card"),
    (1, "network_module"),
    (2, "interface_card"),
    (3, "pt_router_module"),
    (4, "pt_switch_module"),
    (5, "pt_cloud_module"),
    (6, "pt_repeater_module"),
    (7, "pt_host_module"),
    (8, "pt_modem_module"),
    (9, "pt_laptop_module"),
    (10, "pt_tvmodule"),
    (11, "ip_phone_power_adapter"),
    (12, "pt_tablet_pcmodule"),
    (13, "pt_pda_module"),
    (14, "pt_wireless_end_device_module"),
    (15, "pt_wired_end_device_module"),
    (16, "trs35"),
    (17, "usb"),
    (18, "non_removable_module"),
    (19, "asamodule"),
    (20, "asapower_adapter"),
    (21, "pt_cell_tower_module"),
    (22, "pt_ioe_module"),
    (23, "pt_ioe_network_module"),
    (24, "pt_ioe_analog_module"),
    (25, "pt_ioe_digital_module"),
    (26, "pt_ioe_custom_iomodule"),
    (27, "pt_ioe_power_adapter"),
    (28, "pt_ioe_mcu_component_power_adapter"),
    (29, "pt_router_power_adapter"),
    (30, "sfp_module"),
    (31, "access_point_power_adaptor"),
    (32, "non_removable_interface_card"),
    (33, "hot_swappable_power_module"),
    (34, "meraki_power_adaptor"),
    (35, "network_controller_network_module"),
    (36, "isadcpower_adapter_a"),
    (37, "isadcpower_adapter_b"),
    // Missing from the 9.0.1 ModuleType enum; named after the only modules of each type.
    (38, "plc_power_adapter"),
    (39, "hmi_power_adapter"),
    (2000, "custom_module_type"),
];

// com.cisco.pt.ipc.enums.ConnectType values, named the way the CCNA curriculum names cables.
pub(crate) const CABLE_KINDS: &[(i64, &str)] = &[
    (8100, "straight"),
    (8101, "cross"),
    (8102, "rollover"),
    (8103, "fiber"),
    (8104, "phone"),
    (8105, "cable"),
    (8106, "serial"),
    (8107, "auto"),
    (8108, "console"),
    (8109, "wireless"),
    (8110, "coaxial"),
    (8111, "octal"),
    (8112, "cellular"),
    (8113, "usb"),
    (8114, "custom_io"),
    (8115, "bluetooth_paired"),
    (8116, "bluetooth_broadcast"),
    (8117, "fiber_multimode"),
];

pub fn cable_kind(code: i64) -> String {
    name_for(CABLE_KINDS, code)
}

pub fn device_kind(code: i64) -> String {
    name_for(DEVICE_KINDS, code)
}

pub fn module_kind(code: i64) -> String {
    name_for(MODULE_KINDS, code)
}

pub fn runs_ios(kind: &str) -> bool {
    IOS_KINDS.contains(&kind)
}

pub fn device_kind_names() -> impl Iterator<Item = &'static str> {
    DEVICE_KINDS.iter().map(|(_, name)| *name)
}

pub fn module_kind_names() -> impl Iterator<Item = &'static str> {
    MODULE_KINDS.iter().map(|(_, name)| *name)
}

fn name_for(table: &[(i64, &str)], code: i64) -> String {
    table
        .iter()
        .find(|(value, _)| *value == code)
        .map_or_else(|| format!("type_{code}"), |(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_wire_values() {
        assert_eq!(device_kind(0), "router");
        assert_eq!(device_kind(8), "pc");
        assert_eq!(device_kind(16), "multi_layer_switch");
        assert_eq!(device_kind(55), "data_historian");
        assert_eq!(module_kind(2), "interface_card");
        assert_eq!(module_kind(2000), "custom_module_type");
        assert_eq!(cable_kind(8100), "straight");
        assert_eq!(cable_kind(8117), "fiber_multimode");
    }

    #[test]
    fn unknown_codes_stay_visible() {
        assert_eq!(device_kind(52), "type_52");
    }

    #[test]
    fn tables_have_unique_values_and_names() {
        for table in [DEVICE_KINDS, MODULE_KINDS, CABLE_KINDS] {
            let mut values: Vec<_> = table.iter().map(|(value, _)| value).collect();
            let mut names: Vec<_> = table.iter().map(|(_, name)| name).collect();
            values.dedup();
            names.sort_unstable();
            names.dedup();
            assert_eq!(values.len(), table.len());
            assert_eq!(names.len(), table.len());
        }
    }
}
