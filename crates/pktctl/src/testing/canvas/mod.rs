mod activity;
mod catalog;
mod console;
mod desktop;
mod firewall;
mod models;
mod modules;
mod network;
mod options;
mod physical;
mod remote;
mod services;
mod simulation;
mod wireless;
mod workspace;

use std::{
    fmt::Write,
    net::Ipv4Addr,
    sync::{Mutex, MutexGuard, PoisonError},
};

use ptmp::{Call, Event, Value};

pub use activity::ActivityFixture;
use models::{Model, PortKind, model};
pub use remote::Remote;

#[derive(Debug, Clone)]
struct Port {
    name: String,
    kind: PortKind,
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
    dhcp: bool,
    ipv6: Ipv6Settings,
    firewall: bool,
    firewall_v6: bool,
}

#[derive(Debug, Clone, Default)]
struct Ipv6Settings {
    enabled: bool,
    auto_config: bool,
    addresses: Vec<(std::net::Ipv6Addr, i32)>,
    gateway: Option<std::net::Ipv6Addr>,
    dns: Option<std::net::Ipv6Addr>,
}

impl Port {
    fn new(name: String, kind: PortKind) -> Self {
        Self {
            name,
            kind,
            ip: Ipv4Addr::UNSPECIFIED,
            mask: Ipv4Addr::UNSPECIFIED,
            gateway: Ipv4Addr::UNSPECIFIED,
            dns: Ipv4Addr::UNSPECIFIED,
            dhcp: false,
            ipv6: Ipv6Settings::default(),
            firewall: false,
            firewall_v6: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Device {
    name: String,
    physical_name: String,
    model: &'static str,
    x: f64,
    y: f64,
    ports: Vec<Port>,
    cli: Vec<(String, String)>,
    console_prompt: String,
    console_mode: &'static str,
    paged: Option<String>,
    running: Option<String>,
    access_radio: Option<wireless::Radio>,
    client: Option<wireless::Client>,
    services: Option<services::Services>,
    powered: bool,
    cards: Vec<Option<&'static str>>,
    desktop: Option<desktop::Desktop>,
    acls: firewall::Acls,
}

impl Device {
    fn new(model: &'static Model, name: String, x: f64, y: f64) -> Self {
        let ports = (model.ports)()
            .into_iter()
            .map(|(name, kind)| Port::new(name, kind))
            .collect();
        Self {
            physical_name: name.clone(),
            name,
            model: model.name,
            x,
            y,
            ports,
            cli: Vec::new(),
            console_prompt: model.first_prompt.to_owned(),
            console_mode: "user",
            paged: None,
            running: None,
            access_radio: model
                .has_port(models::PortKind::AccessRadio)
                .then(wireless::Radio::default),
            client: model
                .has_port(models::PortKind::ClientRadio)
                .then(wireless::Client::default),
            services: (model.class == "Server").then(services::Services::default),
            powered: true,
            cards: vec![None; model.card_slots],
            desktop: matches!(model.class, "Pc" | "Server").then(desktop::Desktop::default),
            acls: firewall::Acls::default(),
        }
    }

    fn model(&self) -> &'static Model {
        model(self.model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    device: String,
    port: String,
}

#[derive(Debug, Clone)]
struct Link {
    ends: [Endpoint; 2],
    cable: i32,
}

#[derive(Debug, Clone)]
pub(super) struct CanvasNote {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) x: i32,
    pub(super) y: i32,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Network {
    devices: Vec<Device>,
    links: Vec<Link>,
    notes: Vec<CanvasNote>,
    physical: physical::Physical,
}

#[derive(Debug, Default)]
struct State {
    devices: Vec<Device>,
    links: Vec<Link>,
    notes: Vec<CanvasNote>,
    next_note: u32,
    current_file: String,
    files: std::collections::HashMap<String, Network>,
    events: Vec<Event>,
    physical: physical::Physical,
    physical_mode: bool,
    simulation: simulation::Simulation,
    realtime_presses: Vec<String>,
    options: std::collections::BTreeMap<&'static str, bool>,
    activity: Option<activity::Activity>,
    description: String,
    exported: Option<Network>,
}

impl State {
    fn snapshot(&self) -> Network {
        Network {
            devices: self.devices.clone(),
            links: self.links.clone(),
            notes: self.notes.clone(),
            physical: self.physical.clone(),
        }
    }

    fn document(&self) -> String {
        let mut devices = String::new();
        for device in &self.devices {
            let wireless = device
                .client
                .as_ref()
                .map(|client| wireless::profile_xml(&client.profile))
                .unwrap_or_default();
            let name = device.name.replace('&', "&amp;").replace('<', "&lt;");
            let _ = write!(
                devices,
                "<DEVICE><ENGINE><NAME translate=\"true\">{name}</NAME>{wireless}</ENGINE></DEVICE>"
            );
        }
        format!(
            "<PACKETTRACER5><VERSION>9.0.1.0858</VERSION><NETWORK><DEVICES>{devices}</DEVICES></NETWORK>{}</PACKETTRACER5>",
            self.physical.workspace_xml()
        )
    }

    fn load_document(&mut self, xml: &str) -> Result<(), pktfile::PktError> {
        self.physical = physical::Physical::from_nodes(&pktfile::physical_nodes(xml)?);
        for device in &mut self.devices {
            if let Some(client) = device.client.as_mut()
                && let Ok(profile) = pktfile::client_profile(xml, &device.name)
            {
                client.profile = profile;
            }
        }
        wireless::associate(self);
        Ok(())
    }

    fn restore(&mut self, network: Network) {
        self.devices = network.devices;
        self.links = network.links;
        self.notes = network.notes;
        self.physical = network.physical;
    }
}

impl State {
    fn link_at(&self, device: &str, port: &str) -> Option<&Link> {
        self.links.iter().find(|link| {
            link.ends
                .iter()
                .any(|end| end.device == device && end.port == port)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRecord {
    pub a: (String, String),
    pub b: (String, String),
    pub cable: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostAddressing {
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
    pub dhcp: bool,
}

#[derive(Debug, Default)]
pub struct Canvas {
    state: Mutex<State>,
}

impl Canvas {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn device_names(&self) -> Vec<String> {
        self.state()
            .devices
            .iter()
            .map(|device| device.name.clone())
            .collect()
    }

    pub fn position(&self, name: &str) -> Option<(f64, f64)> {
        self.state()
            .devices
            .iter()
            .find(|device| device.name == name)
            .map(|device| (device.x, device.y))
    }

    pub fn links(&self) -> Vec<LinkRecord> {
        self.state()
            .links
            .iter()
            .map(|link| {
                let [a, b] = &link.ends;
                LinkRecord {
                    a: (a.device.clone(), a.port.clone()),
                    b: (b.device.clone(), b.port.clone()),
                    cable: link.cable,
                }
            })
            .collect()
    }

    pub fn console_prompt(&self, device: &str) -> Option<String> {
        self.state()
            .devices
            .iter()
            .find(|candidate| candidate.name == device)
            .map(|device| device.console_prompt.clone())
    }

    pub fn reset_console(&self, device: &str) {
        if let Some(device) = self
            .state()
            .devices
            .iter_mut()
            .find(|candidate| candidate.name == device)
        {
            device
                .model()
                .first_prompt
                .clone_into(&mut device.console_prompt);
            device.console_mode = "user";
        }
    }

    pub fn associate_wireless(&self, device: &str, port: &str) {
        let end = Endpoint {
            device: device.to_owned(),
            port: port.to_owned(),
        };
        self.state().links.push(Link {
            ends: [end.clone(), end],
            cable: 8109,
        });
    }

    pub fn open_activity(&self, path: &str, fixture: ActivityFixture) {
        let mut state = self.state();
        state.activity = Some(activity::Activity::new(fixture));
        path.clone_into(&mut state.current_file);
    }

    pub fn is_physical_mode(&self) -> bool {
        self.state().physical_mode
    }

    pub fn realtime_presses(&self) -> Vec<String> {
        self.state().realtime_presses.clone()
    }

    pub fn physical_parent(&self, device: &str) -> Option<String> {
        let state = self.state();
        let physical_name = &state
            .devices
            .iter()
            .find(|candidate| candidate.name == device)?
            .physical_name;
        state.physical.parent_of_device(physical_name)
    }

    pub fn installed_cards(&self, device: &str) -> Vec<Option<String>> {
        self.state()
            .devices
            .iter()
            .find(|candidate| candidate.name == device)
            .map(|device| {
                device
                    .cards
                    .iter()
                    .map(|card| card.map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_powered(&self, device: &str) -> Option<bool> {
        self.state()
            .devices
            .iter()
            .find(|candidate| candidate.name == device)
            .map(|device| device.powered)
    }

    pub fn cli_history(&self, device: &str) -> Vec<(String, String)> {
        self.state()
            .devices
            .iter()
            .find(|candidate| candidate.name == device)
            .map(|device| device.cli.clone())
            .unwrap_or_default()
    }

    pub fn host_addressing(&self, device: &str, port: &str) -> Option<HostAddressing> {
        let state = self.state();
        let port = state
            .devices
            .iter()
            .find(|candidate| candidate.name == device)?
            .ports
            .iter()
            .find(|candidate| candidate.name == port)?;
        Some(HostAddressing {
            ip: port.ip,
            mask: port.mask,
            gateway: port.gateway,
            dns: port.dns,
            dhcp: port.dhcp,
        })
    }

    pub fn handle(&self, call: &Call) -> Result<Value, Remote> {
        let steps = call.steps();
        let mut state = self.state();
        match steps[0].method.as_str() {
            "hardwareFactory" => catalog::handle(&steps[1..]),
            "network" => network::handle(&mut state, &steps[1..]),
            "appWindow" => workspace::handle(&mut state, &steps[1..]),
            "systemFileManager" => workspace::files(&state, &steps[1..]),
            "simulation" => simulation::handle(&mut state, &steps[1..]),
            "options" => options::handle(&mut state.options, &steps[1..]),
            "getObjectByUuid" => {
                let uuid = steps[0]
                    .args
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match physical::by_uuid(&state, uuid) {
                    Some(id) => physical::object(&mut state, id, &steps[1..]),
                    None => Err(Remote::missing("IPCObject")),
                }
            }
            other => Err(Remote::unknown_method("IPC", other)),
        }
    }

    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.state().events)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
