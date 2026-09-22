use ptmp::{Step, TypeCode, Value};

use super::{
    Device, Endpoint, Link, State,
    models::MODELS,
    remote::{Remote, check_args, number, qstring_arg},
};

const WORKSPACE: &str = "LogicalWorkspace";
const STRAIGHT: i32 = 8100;
const CROSS: i32 = 8101;
const FIBER: i32 = 8103;
const SERIAL: i32 = 8106;
const FIBER_MULTIMODE: i32 = 8117;

pub(super) fn handle(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let methods: Vec<&str> = steps.iter().map(|step| step.method.as_str()).collect();
    let ["getActiveWorkspace", "getLogicalWorkspace", _] = methods.as_slice() else {
        return Err(Remote::unknown_method(
            "AppWindow",
            methods.first().copied().unwrap_or(""),
        ));
    };
    let step = &steps[2];
    match step.method.as_str() {
        "addDevice" => add_device(state, step),
        "removeDevice" => {
            let name = qstring_arg(step, WORKSPACE)?.to_owned();
            let before = state.devices.len();
            state.devices.retain(|device| device.name != name);
            state
                .links
                .retain(|link| link.ends.iter().all(|end| end.device != name));
            Ok(Value::Bool(state.devices.len() < before))
        }
        "createLink" => create_link(state, step),
        "deleteLink" => {
            check_args(step, WORKSPACE, &[TypeCode::QString, TypeCode::String])?;
            let device = step.args[0].as_str().unwrap_or_default();
            let port = step.args[1].as_str().unwrap_or_default();
            let before = state.links.len();
            state.links.retain(|link| {
                link.ends
                    .iter()
                    .all(|end| end.device != device || end.port != port)
            });
            Ok(Value::Bool(state.links.len() < before))
        }
        other => Err(Remote::unknown_method(WORKSPACE, other)),
    }
}

fn add_device(state: &mut State, step: &Step) -> Result<Value, Remote> {
    check_args(
        step,
        WORKSPACE,
        &[
            TypeCode::Int,
            TypeCode::String,
            TypeCode::Double,
            TypeCode::Double,
        ],
    )?;
    let type_code = step.args[0].as_i64().unwrap_or_default();
    let wanted = step.args[1].as_str().unwrap_or_default();
    let Some(model) = MODELS
        .iter()
        .find(|model| model.name == wanted && i64::from(model.type_code) == type_code)
    else {
        return Ok(Value::qstring(""));
    };
    let name = (0..=state.devices.len())
        .map(|n| format!("{}{n}", model.prefix))
        .find(|name| state.devices.iter().all(|device| &device.name != name))
        .expect("an unused name always exists");
    state.devices.push(Device::new(
        model,
        name.clone(),
        number(&step.args[2]),
        number(&step.args[3]),
    ));
    Ok(Value::qstring(name))
}

fn create_link(state: &mut State, step: &Step) -> Result<Value, Remote> {
    check_args(
        step,
        WORKSPACE,
        &[
            TypeCode::QString,
            TypeCode::String,
            TypeCode::QString,
            TypeCode::String,
            TypeCode::Int,
        ],
    )?;
    let text = |index: usize| step.args[index].as_str().unwrap_or_default().to_owned();
    let ends = [
        Endpoint {
            device: text(0),
            port: text(1),
        },
        Endpoint {
            device: text(2),
            port: text(3),
        },
    ];
    let cable = i32::try_from(step.args[4].as_i64().unwrap_or_default()).unwrap_or_default();

    let exists = |end: &Endpoint| {
        state.devices.iter().any(|device| {
            device.name == end.device && device.ports.iter().any(|port| port.name == end.port)
        })
    };
    let free = |end: &Endpoint| state.link_at(&end.device, &end.port).is_none();
    let wired = ends.iter().all(|end| end.port != "Bluetooth");
    let serial = ends.iter().any(|end| end.port.starts_with("Serial"));
    let compatible = if serial {
        cable == SERIAL
    } else {
        [STRAIGHT, CROSS, FIBER, FIBER_MULTIMODE].contains(&cable)
    };

    if ends.iter().all(exists) && ends.iter().all(free) && wired && compatible && ends[0] != ends[1]
    {
        state.links.push(Link { ends, cable });
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}
