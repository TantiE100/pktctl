use ptmp::{Step, TypeCode, Value};

use super::{
    CanvasNote, Device, Endpoint, Link, Network, State, activity,
    models::MODELS,
    physical,
    remote::{Remote, check_args, no_args, number, qstring_arg},
    simulation, wireless,
};

const WORKSPACE: &str = "LogicalWorkspace";
const APP_WINDOW: &str = "AppWindow";
const STRAIGHT: i32 = 8100;
const CROSS: i32 = 8101;
const FIBER: i32 = 8103;
const SERIAL: i32 = 8106;
const FIBER_MULTIMODE: i32 = 8117;

pub(super) fn handle(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let methods: Vec<&str> = steps.iter().map(|step| step.method.as_str()).collect();
    match methods.as_slice() {
        ["getActiveWorkspace", "getLogicalWorkspace", _] => logical(state, &steps[2]),
        ["getActiveWorkspace", "getRootPhysicalObject", ..] => {
            physical::object(state, 0, &steps[2..])
        }
        ["getPhysicalToolbar", ..] => physical::toolbar(state, &steps[1..]),
        ["getPLSwitch", mode] => {
            match *mode {
                "showPhysicalMode" => state.physical_mode = true,
                "showLogicalMode" => state.physical_mode = false,
                other => return Err(Remote::unknown_method("PLSwitch", other)),
            }
            Ok(Value::Void)
        }
        ["getActiveWorkspace", "setLogicalBackgroundPath"] => {
            check_args(&steps[1], "Workspace", &[TypeCode::QString, TypeCode::Bool])?;
            steps[1].args[0]
                .as_str()
                .unwrap_or_default()
                .clone_into(&mut state.logical_background);
            Ok(Value::Void)
        }
        ["isPhysicalMode"] => Ok(Value::Bool(state.physical_mode)),
        ["fileSaveToBytes"] => {
            state.exported = Some(state.snapshot());
            Ok(Value::Bytes(
                pktfile::encode(&state.document()).expect("canvas XML encodes"),
            ))
        }
        ["getRealtimeToolbar", button @ "fastForwardTime"] => {
            no_args(&steps[1], "RealtimeToolbar")?;
            state.realtime_presses.push((*button).to_owned());
            Ok(Value::Void)
        }
        ["getUserCreatedPDU", "addSimplePdu"] => simulation::add_simple_pdu(state, &steps[1]),
        ["getActiveFile", "getSavedFilename"] => Ok(Value::qstring(&state.current_file)),
        ["getActiveFile", _] => {
            let State {
                activity,
                description,
                ..
            } = state;
            activity::handle(activity.as_mut(), description, &steps[1])
        }
        ["fileSaveAsNoPrompt"] => {
            check_args(&steps[0], APP_WINDOW, &[TypeCode::QString, TypeCode::Bool])?;
            let path = steps[0].args[0].as_str().unwrap_or_default().to_owned();
            let snapshot = state.snapshot();
            if std::path::Path::new(&path)
                .parent()
                .is_some_and(std::path::Path::is_dir)
            {
                let file = pktfile::encode(&state.document()).expect("canvas XML encodes");
                std::fs::write(&path, file).expect("canvas can write real files");
            }
            state.files.insert(path.clone(), snapshot);
            state.current_file = path;
            Ok(Value::Void)
        }
        ["fileNew"] => {
            check_args(&steps[0], APP_WINDOW, &[TypeCode::Bool])?;
            state.restore(Network::default());
            state.current_file.clear();
            state.activity = None;
            Ok(Value::Bool(true))
        }
        ["fileOpen"] => {
            let path = qstring_arg(&steps[0], APP_WINDOW)?.to_owned();
            let on_disk = std::fs::read(&path)
                .ok()
                .map(|bytes| pktfile::decode(&bytes));
            let remembered = state.files.get(&path).cloned().or_else(|| {
                on_disk
                    .as_ref()
                    .filter(|decoded| decoded.is_ok())
                    .and_then(|_| state.exported.clone())
            });
            match (remembered, on_disk) {
                (_, Some(Err(_))) => Ok(Value::Int(3)),
                (Some(network), on_disk) => {
                    state.restore(network);
                    if let Some(Ok(xml)) = on_disk
                        && state.load_document(&xml).is_err()
                    {
                        return Ok(Value::Int(3));
                    }
                    state.current_file = path;
                    Ok(Value::Int(0))
                }
                (None, _) => Ok(Value::Int(6)),
            }
        }
        _ => Err(Remote::unknown_method(
            APP_WINDOW,
            methods.first().copied().unwrap_or(""),
        )),
    }
}

pub(super) fn files(state: &State, steps: &[Step]) -> Result<Value, Remote> {
    const CLASS: &str = "SystemFileManager";
    let [step] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    let path = qstring_arg(step, CLASS)?;
    let exists = state.files.contains_key(path);
    match step.method.as_str() {
        "fileExists" => Ok(Value::Bool(exists || std::path::Path::new(path).is_file())),
        "getFileSize" => Ok(Value::Int(if exists { 4096 } else { -1 })),
        other => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn logical(state: &mut State, step: &Step) -> Result<Value, Remote> {
    match step.method.as_str() {
        "addDevice" => add_device(state, step),
        "removeDevice" => {
            let name = qstring_arg(step, WORKSPACE)?.to_owned();
            let before = state.devices.len();
            if let Some(device) = state.devices.iter().find(|device| device.name == name) {
                state.physical.remove_device(&device.physical_name);
            }
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
        "getIncNoteZOrder" => Ok(Value::Double(f64::from(state.next_note) + 1.0)),
        "addNote" => {
            check_args(
                step,
                WORKSPACE,
                &[
                    TypeCode::Int,
                    TypeCode::Int,
                    TypeCode::Double,
                    TypeCode::QString,
                ],
            )?;
            state.next_note += 1;
            let id = format!("{{00000000-0000-0000-0000-{:012}}}", state.next_note);
            state.notes.push(CanvasNote {
                id: id.clone(),
                text: step.args[3].as_str().unwrap_or_default().to_owned(),
                x: int(&step.args[0]),
                y: int(&step.args[1]),
            });
            Ok(Value::Uuid(id))
        }
        "getCanvasNoteIds" => Ok(Value::Vector {
            element: TypeCode::Uuid,
            items: all_notes(state)
                .into_iter()
                .map(|note| Value::Uuid(note.id))
                .collect(),
        }),
        "getCanvasNoteText" | "getCanvasItemRealX" | "getCanvasItemRealY" => {
            check_args(step, WORKSPACE, &[TypeCode::Uuid])?;
            let id = step.args[0].as_str().unwrap_or_default();
            let note = all_notes(state)
                .into_iter()
                .find(|note| note.id == id)
                .ok_or_else(|| Remote::missing("CanvasItem"))?;
            Ok(match step.method.as_str() {
                "getCanvasNoteText" => Value::qstring(&note.text),
                "getCanvasItemRealX" => Value::Int(note.x),
                _ => Value::Int(note.y),
            })
        }
        "removeCanvasItem" => {
            check_args(step, WORKSPACE, &[TypeCode::Uuid])?;
            let id = step.args[0].as_str().unwrap_or_default();
            let before = state.notes.len();
            state.notes.retain(|note| note.id != id);
            Ok(Value::Bool(state.notes.len() < before))
        }
        "getWorkspaceImage" => {
            qstring_arg(step, WORKSPACE)?;
            Ok(Value::Bytes(
                b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec(),
            ))
        }
        other => Err(Remote::unknown_method(WORKSPACE, other)),
    }
}

fn all_notes(state: &State) -> Vec<CanvasNote> {
    let labels = state.links.iter().enumerate().flat_map(|(index, link)| {
        link.ends
            .iter()
            .enumerate()
            .map(move |(end, endpoint)| CanvasNote {
                id: format!("{{11111111-0000-0000-0000-{index:06}{end:06}}}"),
                text: label(&endpoint.port),
                x: 0,
                y: 0,
            })
    });
    state.notes.iter().cloned().chain(labels).collect()
}

fn label(port: &str) -> String {
    [
        ("GigabitEthernet", "Gig"),
        ("FastEthernet", "Fa"),
        ("Serial", "Se"),
    ]
    .iter()
    .find_map(|(full, short)| port.strip_prefix(full).map(|rest| format!("{short}{rest}")))
    .unwrap_or_else(|| port.to_owned())
}

fn int(value: &Value) -> i32 {
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .unwrap_or_default()
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
    state.physical.place_device(&name, model.class != "Pc");
    wireless::associate(state);
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
