mod catalog;
mod models;
mod network;
mod remote;
mod workspace;

use std::{
    net::Ipv4Addr,
    sync::{Mutex, MutexGuard, PoisonError},
};

use ptmp::{Call, Value};

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
}

#[derive(Debug, Clone)]
struct Device {
    name: String,
    model: &'static str,
    x: f64,
    y: f64,
    ports: Vec<Port>,
}

impl Device {
    fn new(model: &'static Model, name: String, x: f64, y: f64) -> Self {
        let ports = (model.ports)()
            .into_iter()
            .map(|(name, kind)| Port {
                name,
                kind,
                ip: Ipv4Addr::UNSPECIFIED,
                mask: Ipv4Addr::UNSPECIFIED,
                gateway: Ipv4Addr::UNSPECIFIED,
                dns: Ipv4Addr::UNSPECIFIED,
                dhcp: false,
            })
            .collect();
        Self {
            name,
            model: model.name,
            x,
            y,
            ports,
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

#[derive(Debug, Default)]
struct State {
    devices: Vec<Device>,
    links: Vec<Link>,
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
            other => Err(Remote::unknown_method("IPC", other)),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
