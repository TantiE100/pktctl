use std::net::Ipv4Addr;

use ptmp::{Step, TypeCode, Value};

use super::{
    Endpoint, Port, State, console, modules,
    remote::{Remote, check_args, count, int_arg, no_args, number, qstring_arg, string_arg},
};

pub(super) fn handle(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let [first, rest @ ..] = steps else {
        return Err(Remote::unknown_method("Network", ""));
    };
    match first.method.as_str() {
        "getDeviceCount" => Ok(count(state.devices.len())),
        "getLinkCount" => Ok(count(state.links.len())),
        "getDeviceAt" => {
            let index = int_arg(first, "Network")?;
            let index = usize::try_from(index)
                .ok()
                .filter(|index| *index < state.devices.len())
                .ok_or_else(|| Remote::missing("Device"))?;
            device(state, index, rest)
        }
        "getDevice" => {
            let name = qstring_arg(first, "Network")?;
            let index = state
                .devices
                .iter()
                .position(|device| device.name == name)
                .ok_or_else(|| Remote::missing("Device"))?;
            device(state, index, rest)
        }
        "getLinkAt" => {
            let index = int_arg(first, "Network")?;
            let link = usize::try_from(index)
                .ok()
                .and_then(|index| state.links.get(index))
                .ok_or_else(|| Remote::missing("Link"))?
                .clone();
            link_call(state, &link.ends, link.cable, rest)
        }
        other => Err(Remote::unknown_method("Network", other)),
    }
}

fn device(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    let class = state.devices[index].model().class;
    let [step, rest @ ..] = steps else {
        return Err(Remote::unknown_method(class, ""));
    };
    match (step.method.as_str(), rest) {
        ("getPortAt", rest) => {
            let name = usize::try_from(int_arg(step, class)?)
                .ok()
                .and_then(|port| state.devices[index].ports.get(port))
                .ok_or_else(|| Remote::missing("Port"))?
                .name
                .clone();
            port(state, index, &name, rest)
        }
        ("getRootModule", rest) => {
            no_args(step, class)?;
            modules::tree(&state.devices[index], rest)
        }
        ("getCommandLine", rest) if state.devices[index].model().ios => {
            no_args(step, class)?;
            console::handle(state, index, rest)
        }
        ("getPort", rest) => {
            let name = string_arg(step, class)?.to_owned();
            port(state, index, &name, rest)
        }
        (_, []) => device_attribute(state, index, step),
        (other, _) => Err(Remote::unknown_method(class, other)),
    }
}

fn device_attribute(state: &mut State, index: usize, step: &Step) -> Result<Value, Remote> {
    let class = state.devices[index].model().class;
    let ios = state.devices[index].model().ios;
    let getter = || no_args(step, class);
    match step.method.as_str() {
        "getName" => getter().map(|()| Value::qstring(&state.devices[index].name)),
        "getModel" => getter().map(|()| Value::qstring(state.devices[index].model)),
        "getType" => getter().map(|()| Value::Int(state.devices[index].model().type_code)),
        "getClassName" => getter().map(|()| Value::qstring(class)),
        "getCenterXCoordinate" => getter().map(|()| Value::Double(state.devices[index].x)),
        "getCenterYCoordinate" => getter().map(|()| Value::Double(state.devices[index].y)),
        "getPortCount" => getter().map(|()| count(state.devices[index].ports.len())),
        "skipBoot" if ios => getter().map(|()| Value::Void),
        "getPower" => getter().map(|()| Value::Bool(state.devices[index].powered)),
        "setPower" => {
            check_args(step, class, &[TypeCode::Bool])?;
            let device = &mut state.devices[index];
            let on = step.args[0].as_bool().unwrap_or_default();
            if on && !device.powered {
                device
                    .model()
                    .first_prompt
                    .clone_into(&mut device.console_prompt);
                device.console_mode = "user";
                device.paged = None;
            }
            device.powered = on;
            Ok(Value::Void)
        }
        "getSupportedModule" => getter().map(|()| modules::supported(&state.devices[index])),
        "addModule" => modules::add(&mut state.devices[index], step),
        "removeModule" => {
            let removed = modules::remove(&mut state.devices[index], step)?;
            let device = &state.devices[index];
            state.links.retain(|link| {
                link.ends.iter().all(|end| {
                    end.device != device.name
                        || device.ports.iter().any(|port| port.name == end.port)
                })
            });
            Ok(removed)
        }
        "enterCommand" if ios => {
            check_args(step, class, &[TypeCode::String, TypeCode::String])?;
            let command = step.args[0].as_str().unwrap_or_default().to_owned();
            let mode = step.args[1].as_str().unwrap_or_default().to_owned();
            let (status, output) = if command.starts_with("bogus") {
                (2, "\n")
            } else if command == "write memory" {
                (0, "Building configuration...\n[OK]\n\n")
            } else {
                (0, "\n")
            };
            state.devices[index].cli.push((mode, command));
            Ok(Value::Pair(
                Box::new(Value::Int(status)),
                Box::new(Value::string(output)),
            ))
        }
        "setDhcpFlag" if !ios => {
            check_args(step, class, &[TypeCode::Bool])?;
            if step.args[0].as_bool() == Some(true) {
                for port in &mut state.devices[index].ports {
                    port.ip = Ipv4Addr::UNSPECIFIED;
                    port.mask = Ipv4Addr::UNSPECIFIED;
                }
            }
            Ok(Value::Void)
        }
        "setName" => {
            let new_name = qstring_arg(step, class)?.to_owned();
            let old_name = std::mem::replace(&mut state.devices[index].name, new_name.clone());
            for end in state.links.iter_mut().flat_map(|link| link.ends.iter_mut()) {
                if end.device == old_name {
                    end.device.clone_from(&new_name);
                }
            }
            Ok(Value::Void)
        }
        "moveToLocationCentered" => {
            check_args(step, class, &[TypeCode::Int, TypeCode::Int])?;
            state.devices[index].x = number(&step.args[0]);
            state.devices[index].y = number(&step.args[1]);
            Ok(Value::Bool(true))
        }
        other => Err(Remote::unknown_method(class, other)),
    }
}

fn port(state: &mut State, index: usize, name: &str, steps: &[Step]) -> Result<Value, Remote> {
    let device_name = state.devices[index].name.clone();
    let Some(position) = state.devices[index]
        .ports
        .iter()
        .position(|port| port.name == name)
    else {
        return Err(Remote::missing("Port"));
    };
    let port = state.devices[index].ports[position].clone();
    let class = port.kind.class();
    let linked = state.link_at(&device_name, name).cloned();
    match steps {
        [step] if port.kind.has_ip() && step.method.starts_with("set") => {
            let port = &mut state.devices[index].ports[position];
            set_address(port, step, class)
        }
        [step] => {
            no_args(step, class)?;
            match step.method.as_str() {
                "getName" => Ok(Value::string(&port.name)),
                "isPortUp" | "isProtocolUp" => Ok(Value::Bool(linked.is_some())),
                "getIpAddress" if port.kind.has_ip() => Ok(Value::Ip(port.ip)),
                "getSubnetMask" if port.kind.has_ip() => Ok(Value::Ip(port.mask)),
                "isDhcpClientOn" if port.kind.has_ip() => Ok(Value::Bool(port.dhcp)),
                other => Err(Remote::unknown_method(class, other)),
            }
        }
        [link, rest @ ..] if link.method == "getLink" => {
            no_args(link, class)?;
            let link = linked.ok_or_else(|| Remote::missing("Link"))?;
            link_call(state, &link.ends, link.cable, rest)
        }
        [other, ..] => Err(Remote::unknown_method(class, &other.method)),
        [] => Err(Remote::unknown_method(class, "")),
    }
}

fn set_address(port: &mut Port, step: &Step, class: &str) -> Result<Value, Remote> {
    match step.method.as_str() {
        "setIpSubnetMask" => {
            check_args(step, class, &[TypeCode::Ip, TypeCode::Ip])?;
            port.ip = step.args[0].as_ip().unwrap_or(Ipv4Addr::UNSPECIFIED);
            port.mask = step.args[1].as_ip().unwrap_or(Ipv4Addr::UNSPECIFIED);
        }
        "setDefaultGateway" => {
            check_args(step, class, &[TypeCode::Ip])?;
            port.gateway = step.args[0].as_ip().unwrap_or(Ipv4Addr::UNSPECIFIED);
        }
        "setDnsServerIp" => {
            check_args(step, class, &[TypeCode::Ip])?;
            port.dns = step.args[0].as_ip().unwrap_or(Ipv4Addr::UNSPECIFIED);
        }
        "setDhcpClientFlag" => {
            check_args(step, class, &[TypeCode::Bool])?;
            port.dhcp = step.args[0].as_bool().unwrap_or_default();
        }
        other => return Err(Remote::unknown_method(class, other)),
    }
    Ok(Value::Void)
}

fn link_call(
    state: &mut State,
    ends: &[Endpoint; 2],
    cable: i32,
    steps: &[Step],
) -> Result<Value, Remote> {
    match steps {
        [step] if step.method == "getConnectionType" => Ok(Value::Int(cable)),
        [end, rest @ ..] if end.method == "getPort1" || end.method == "getPort2" => {
            let end = &ends[usize::from(end.method == "getPort2")];
            match rest {
                [owner, name] if owner.method == "getOwnerDevice" && name.method == "getName" => {
                    Ok(Value::qstring(&end.device))
                }
                _ => {
                    let index = state
                        .devices
                        .iter()
                        .position(|device| device.name == end.device)
                        .ok_or_else(|| Remote::missing("Device"))?;
                    let port_name = end.port.clone();
                    port(state, index, &port_name, rest)
                }
            }
        }
        [other, ..] => Err(Remote::unknown_method("Cable", &other.method)),
        [] => Err(Remote::unknown_method("Cable", "")),
    }
}
