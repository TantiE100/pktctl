use std::sync::{Mutex, MutexGuard, PoisonError};

use ptmp::{Call, Step, TypeCode, Value};

use crate::packet_tracer::PtError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub class: String,
    pub message: String,
}

impl Remote {
    fn new(class: &str, message: impl Into<String>) -> Self {
        Self {
            class: class.to_owned(),
            message: message.into(),
        }
    }

    fn missing(class: &str) -> Self {
        Self::new(class, "IPC Cache entry: ")
    }

    fn unknown_method(class: &str, method: &str) -> Self {
        Self::new(class, format!(r#"IPC call "{method}" not found"#))
    }

    fn invalid_arguments(class: &str, method: &str) -> Self {
        Self::new(
            class,
            format!(r#"Invalid arguments for IPC call "{method}""#),
        )
    }
}

impl From<Remote> for PtError {
    fn from(remote: Remote) -> Self {
        ptmp::Error::Remote {
            class: remote.class,
            message: remote.message,
        }
        .into()
    }
}

struct Model {
    name: &'static str,
    type_code: i32,
    class: &'static str,
    prefix: &'static str,
    ios: bool,
}

const MODELS: &[Model] = &[
    Model {
        name: "2911",
        type_code: 0,
        class: "Router",
        prefix: "Router",
        ios: true,
    },
    Model {
        name: "2960-24TT",
        type_code: 1,
        class: "CiscoDevice",
        prefix: "Switch",
        ios: true,
    },
    Model {
        name: "3560-24PS",
        type_code: 16,
        class: "Router",
        prefix: "Multilayer Switch",
        ios: true,
    },
    Model {
        name: "PC-PT",
        type_code: 8,
        class: "Pc",
        prefix: "PC",
        ios: false,
    },
    Model {
        name: "Server-PT",
        type_code: 9,
        class: "Server",
        prefix: "Server",
        ios: false,
    },
];

const MODULES: &[(&str, i32)] = &[("HWIC-2T", 2), ("NIM-2T", 2), ("NM-1FE-TX", 1)];

#[derive(Debug, Clone)]
struct Device {
    name: String,
    model: &'static str,
    x: f64,
    y: f64,
}

impl Device {
    fn model(&self) -> &'static Model {
        MODELS
            .iter()
            .find(|model| model.name == self.model)
            .expect("devices are only created from known models")
    }
}

#[derive(Debug, Default)]
struct State {
    devices: Vec<Device>,
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

    pub fn handle(&self, call: &Call) -> Result<Value, Remote> {
        let steps = call.steps();
        let mut state = self.state();
        match steps[0].method.as_str() {
            "hardwareFactory" => catalog(&steps[1..]),
            "network" => network(&mut state, &steps[1..]),
            "appWindow" => app_window(&mut state, &steps[1..]),
            other => Err(Remote::unknown_method("IPC", other)),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn catalog(steps: &[Step]) -> Result<Value, Remote> {
    let (factory, rest) = split(steps, "HardwareFactory")?;
    let table: Vec<(&str, i32)> = match factory.method.as_str() {
        "devices" => MODELS
            .iter()
            .map(|model| (model.name, model.type_code))
            .collect(),
        "modules" => MODULES.to_vec(),
        other => return Err(Remote::unknown_method("HardwareFactory", other)),
    };
    let (query, rest) = split(rest, "DeviceFactory")?;
    match (query.method.as_str(), rest) {
        (count, []) if count.ends_with("Count") => Ok(Value::Int(len(table.len()))),
        (at, [attribute]) if at.starts_with("getAvailable") => {
            let index = int_arg(query, "DeviceFactory")?;
            let (model, type_code) = usize::try_from(index)
                .ok()
                .and_then(|index| table.get(index))
                .ok_or_else(|| Remote::missing("DeviceDescriptor"))?;
            match attribute.method.as_str() {
                "getModel" => Ok(Value::qstring(*model)),
                "getType" => Ok(Value::Int(*type_code)),
                other => Err(Remote::unknown_method("DeviceDescriptor", other)),
            }
        }
        (other, _) => Err(Remote::unknown_method("DeviceFactory", other)),
    }
}

fn network(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let (first, rest) = split(steps, "Network")?;
    match first.method.as_str() {
        "getDeviceCount" => Ok(Value::Int(len(state.devices.len()))),
        "getLinkCount" => Ok(Value::Int(0)),
        "getDeviceAt" => {
            let index = int_arg(first, "Network")?;
            let index = usize::try_from(index)
                .ok()
                .filter(|index| *index < state.devices.len())
                .ok_or_else(|| Remote::missing("Device"))?;
            device_call(state, index, rest)
        }
        "getDevice" => {
            let name = qstring_arg(first, "Network")?;
            let index = state
                .devices
                .iter()
                .position(|device| device.name == name)
                .ok_or_else(|| Remote::missing("Device"))?;
            device_call(state, index, rest)
        }
        other => Err(Remote::unknown_method("Network", other)),
    }
}

fn device_call(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    let class = state.devices[index].model().class;
    let [step] = steps else {
        return Err(Remote::unknown_method(
            class,
            steps.first().map_or("", |step| step.method.as_str()),
        ));
    };
    let device = &mut state.devices[index];
    match step.method.as_str() {
        "getName" => Ok(Value::qstring(&device.name)),
        "getModel" => Ok(Value::qstring(device.model)),
        "getType" => Ok(Value::Int(device.model().type_code)),
        "getClassName" => Ok(Value::qstring(class)),
        "getCenterXCoordinate" => Ok(Value::Double(device.x)),
        "getCenterYCoordinate" => Ok(Value::Double(device.y)),
        "skipBoot" if device.model().ios => Ok(Value::Void),
        "setName" => {
            let name = qstring_arg(step, class)?;
            name.clone_into(&mut device.name);
            Ok(Value::Void)
        }
        "moveToLocationCentered" => {
            check_args(step, class, &[TypeCode::Int, TypeCode::Int])?;
            device.x = number(&step.args[0]);
            device.y = number(&step.args[1]);
            Ok(Value::Bool(true))
        }
        other => Err(Remote::unknown_method(class, other)),
    }
}

const WORKSPACE: &str = "LogicalWorkspace";

fn app_window(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let methods: Vec<&str> = steps.iter().map(|step| step.method.as_str()).collect();
    let ["getActiveWorkspace", "getLogicalWorkspace", action] = methods.as_slice() else {
        return Err(Remote::unknown_method(
            "AppWindow",
            methods.first().copied().unwrap_or(""),
        ));
    };
    let step = &steps[2];
    match *action {
        "addDevice" => {
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
            state.devices.push(Device {
                name: name.clone(),
                model: model.name,
                x: number(&step.args[2]),
                y: number(&step.args[3]),
            });
            Ok(Value::qstring(name))
        }
        "removeDevice" => {
            let name = qstring_arg(step, WORKSPACE)?;
            let before = state.devices.len();
            state.devices.retain(|device| device.name != name);
            Ok(Value::Bool(state.devices.len() < before))
        }
        other => Err(Remote::unknown_method(WORKSPACE, other)),
    }
}

fn split<'a>(steps: &'a [Step], class: &str) -> Result<(&'a Step, &'a [Step]), Remote> {
    steps
        .split_first()
        .ok_or_else(|| Remote::unknown_method(class, ""))
}

fn check_args(step: &Step, class: &str, expected: &[TypeCode]) -> Result<(), Remote> {
    let actual: Vec<TypeCode> = step.args.iter().map(Value::type_code).collect();
    if actual == expected {
        Ok(())
    } else {
        Err(Remote::invalid_arguments(class, &step.method))
    }
}

fn int_arg(step: &Step, class: &str) -> Result<i64, Remote> {
    check_args(step, class, &[TypeCode::Int])?;
    Ok(step.args[0].as_i64().unwrap_or_default())
}

fn qstring_arg<'a>(step: &'a Step, class: &str) -> Result<&'a str, Remote> {
    check_args(step, class, &[TypeCode::QString])?;
    Ok(step.args[0].as_str().unwrap_or_default())
}

fn number(value: &Value) -> f64 {
    match *value {
        Value::Double(number) => number,
        Value::Int(number) => f64::from(number),
        _ => 0.0,
    }
}

fn len(count: usize) -> i32 {
    i32::try_from(count).expect("test canvases stay small")
}
