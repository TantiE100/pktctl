use ptmp::{Step, TypeCode, Value};

use super::{
    Device, Port,
    models::{INTERFACE_CARD, MODULES, NON_REMOVABLE_MODULE, PortKind},
    remote::{Remote, check_args, count, int_arg, no_args, string_arg},
};

const CLASS: &str = "Module";

enum Node {
    Chassis,
    Board,
    Card(usize),
}

pub(super) fn tree(device: &Device, steps: &[Step]) -> Result<Value, Remote> {
    let mut node = Node::Chassis;
    let mut steps = steps;
    while let [step, rest @ ..] = steps {
        if step.method != "getModuleAt" {
            break;
        }
        let index = usize::try_from(int_arg(step, CLASS)?).unwrap_or(usize::MAX);
        node = match node {
            Node::Chassis if index == 0 => Node::Board,
            Node::Board if device.cards.get(index).is_some_and(Option::is_some) => {
                Node::Card(index)
            }
            _ => return Err(Remote::missing(CLASS)),
        };
        steps = rest;
    }

    let slots = match node {
        Node::Chassis => 1,
        Node::Board => device.cards.len(),
        Node::Card(_) => 0,
    };
    match steps {
        [step] => {
            let method = step.method.as_str();
            match method {
                "getSlotCount" | "getModuleCount" => no_args(step, CLASS).map(|()| count(slots)),
                "getSlotTypeAt" => {
                    let index = usize::try_from(int_arg(step, CLASS)?).unwrap_or(usize::MAX);
                    if index >= slots {
                        return Err(Remote::missing(CLASS));
                    }
                    Ok(Value::Int(match node {
                        Node::Chassis => NON_REMOVABLE_MODULE,
                        _ => INTERFACE_CARD,
                    }))
                }
                "getModuleType" => no_args(step, CLASS).map(|()| {
                    Value::Int(match node {
                        Node::Card(_) => INTERFACE_CARD,
                        _ => NON_REMOVABLE_MODULE,
                    })
                }),
                other => Err(Remote::unknown_method(CLASS, other)),
            }
        }
        [descriptor, model]
            if descriptor.method == "getDescriptor" && model.method == "getModel" =>
        {
            let name = match node {
                Node::Card(index) => device.cards[index].unwrap_or_default(),
                _ => device.model,
            };
            Ok(Value::string(name))
        }
        [other, ..] => Err(Remote::unknown_method(CLASS, &other.method)),
        [] => Err(Remote::unknown_method(CLASS, "")),
    }
}

pub(super) fn supported(device: &Device) -> Value {
    Value::Vector {
        element: TypeCode::String,
        items: device
            .model()
            .supported_modules
            .iter()
            .map(|model| {
                Value::string(format!(
                    "{model}:../art/PhysicalView/{model}.png{model} card"
                ))
            })
            .collect(),
    }
}

pub(super) fn add(device: &mut Device, step: &Step) -> Result<Value, Remote> {
    let class = device.model().class;
    check_args(
        step,
        class,
        &[TypeCode::String, TypeCode::Int, TypeCode::String],
    )?;
    let slot = step.args[0].as_str().unwrap_or_default();
    let type_code = step.args[1].as_i64().unwrap_or_default();
    let model = step.args[2].as_str().unwrap_or_default();

    let known = MODULES
        .iter()
        .find(|(name, code)| *name == model && i64::from(*code) == type_code);
    let supported = device.model().supported_modules.contains(&model);
    let Some(index) = card_index(device, slot) else {
        return Ok(Value::Bool(false));
    };
    let Some((name, _)) = known else {
        return Ok(Value::Bool(false));
    };
    if device.powered || !supported || device.cards[index].is_some() {
        return Ok(Value::Bool(false));
    }
    device.cards[index] = Some(name);
    for port in 0..2 {
        device.ports.push(Port::new(
            format!("Serial0/{index}/{port}"),
            PortKind::Router,
        ));
    }
    Ok(Value::Bool(true))
}

pub(super) fn remove(device: &mut Device, step: &Step) -> Result<Value, Remote> {
    let slot = string_arg(step, device.model().class)?.to_owned();
    let Some(index) = card_index(device, &slot) else {
        return Ok(Value::Bool(false));
    };
    if device.powered || device.cards[index].is_none() {
        return Ok(Value::Bool(false));
    }
    device.cards[index] = None;
    let prefix = format!("Serial0/{index}/");
    device.ports.retain(|port| !port.name.starts_with(&prefix));
    Ok(Value::Bool(true))
}

fn card_index(device: &Device, slot: &str) -> Option<usize> {
    let index: usize = slot.strip_prefix("0/")?.parse().ok()?;
    (index < device.cards.len()).then_some(index)
}
