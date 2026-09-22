#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PortKind {
    Router,
    Switch,
    Host,
}

impl PortKind {
    pub(super) fn class(self) -> &'static str {
        match self {
            Self::Router => "RouterPort",
            Self::Switch => "SwitchPort",
            Self::Host => "HostPort",
        }
    }

    pub(super) fn has_ip(self) -> bool {
        !matches!(self, Self::Switch)
    }
}

#[derive(Debug)]
pub(super) struct Model {
    pub(super) name: &'static str,
    pub(super) type_code: i32,
    pub(super) class: &'static str,
    pub(super) prefix: &'static str,
    pub(super) ios: bool,
    pub(super) first_prompt: &'static str,
    pub(super) hostname: &'static str,
    pub(super) ports: fn() -> Vec<(String, PortKind)>,
}

pub(super) const INITIAL_DIALOG: &str =
    "Would you like to enter the initial configuration dialog? [yes/no]: ";

pub(super) const MODELS: &[Model] = &[
    Model {
        name: "2911",
        hostname: "Router",
        first_prompt: INITIAL_DIALOG,
        type_code: 0,
        class: "Router",
        prefix: "Router",
        ios: true,
        ports: router_ports,
    },
    Model {
        name: "2960-24TT",
        hostname: "Switch",
        first_prompt: "",
        type_code: 1,
        class: "CiscoDevice",
        prefix: "Switch",
        ios: true,
        ports: switch_ports,
    },
    Model {
        name: "3560-24PS",
        hostname: "Switch",
        first_prompt: INITIAL_DIALOG,
        type_code: 16,
        class: "Router",
        prefix: "Multilayer Switch",
        ios: true,
        ports: switch_ports,
    },
    Model {
        name: "PC-PT",
        hostname: "PC",
        first_prompt: "",
        type_code: 8,
        class: "Pc",
        prefix: "PC",
        ios: false,
        ports: host_ports,
    },
    Model {
        name: "Server-PT",
        hostname: "Server",
        first_prompt: "",
        type_code: 9,
        class: "Server",
        prefix: "Server",
        ios: false,
        ports: host_ports,
    },
];

pub(super) const MODULES: &[(&str, i32)] = &[("HWIC-2T", 2), ("NIM-2T", 2), ("NM-1FE-TX", 1)];

pub(super) fn model(name: &str) -> &'static Model {
    MODELS
        .iter()
        .find(|model| model.name == name)
        .expect("devices are only created from known models")
}

fn router_ports() -> Vec<(String, PortKind)> {
    std::iter::once("Vlan1".to_owned())
        .chain((0..3).map(|index| format!("GigabitEthernet0/{index}")))
        .map(|name| (name, PortKind::Router))
        .collect()
}

fn switch_ports() -> Vec<(String, PortKind)> {
    (1..=24)
        .map(|index| format!("FastEthernet0/{index}"))
        .chain((1..=2).map(|index| format!("GigabitEthernet0/{index}")))
        .map(|name| (name, PortKind::Switch))
        .chain(std::iter::once(("Vlan1".to_owned(), PortKind::Router)))
        .collect()
}

fn host_ports() -> Vec<(String, PortKind)> {
    ["FastEthernet0", "Bluetooth"]
        .into_iter()
        .map(|name| (name.to_owned(), PortKind::Host))
        .collect()
}
