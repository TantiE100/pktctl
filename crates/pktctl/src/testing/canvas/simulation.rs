use ptmp::{Step, TypeCode, Value};

use super::{
    State,
    remote::{Remote, check_args, int_arg, no_args},
};

const CLASS: &str = "Simulation";
const FRAME: &str = "FrameInstance";
const ICMP: i32 = 0;
const NO_SOURCE: i32 = 20;
const NO_DESTINATION: i32 = 30;

#[derive(Debug, Clone)]
struct Frame {
    time: i32,
    device: String,
    previous: Option<String>,
    source: String,
    destination: String,
    accepted: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Simulation {
    on: bool,
    clock: i64,
    frames: Vec<Frame>,
    pending: Vec<(String, String)>,
}

pub(super) fn handle(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let [step, rest @ ..] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    let simulation = &mut state.simulation;
    match (step.method.as_str(), rest) {
        ("getFrameInstanceAt", rest) => {
            let index = int_arg(step, CLASS)?;
            let frame = usize::try_from(index)
                .ok()
                .and_then(|index| simulation.frames.get(index))
                .ok_or_else(|| Remote::missing(FRAME))?;
            frame_call(frame, rest)
        }
        ("setSimulationMode", []) => {
            check_args(step, CLASS, &[TypeCode::Bool])?;
            simulation.on = step.args[0].as_bool().unwrap_or_default();
            if !simulation.on {
                simulation.frames.clear();
                simulation.pending.clear();
            }
            Ok(Value::Void)
        }
        ("isSimulationMode", []) => no_args(step, CLASS).map(|()| Value::Bool(simulation.on)),
        ("getCurrentSimTime", []) => no_args(step, CLASS).map(|()| Value::Long(simulation.clock)),
        ("getFrameInstanceCount", []) => no_args(step, CLASS)
            .map(|()| Value::Int(i32::try_from(simulation.frames.len()).unwrap_or(i32::MAX))),
        ("forward", []) => {
            no_args(step, CLASS)?;
            simulation.clock += 1;
            let time = i32::try_from(simulation.clock).unwrap_or(i32::MAX);
            for (source, destination) in std::mem::take(&mut simulation.pending) {
                simulation.frames.push(Frame {
                    time,
                    device: destination.clone(),
                    previous: Some(source.clone()),
                    source,
                    destination,
                    accepted: true,
                });
            }
            Ok(Value::Void)
        }
        ("backward", []) => no_args(step, CLASS).map(|()| Value::Void),
        ("resetSimulation", []) => {
            no_args(step, CLASS)?;
            simulation.frames.clear();
            simulation.pending.clear();
            Ok(Value::Void)
        }
        (other, _) => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn frame_call(frame: &Frame, steps: &[Step]) -> Result<Value, Remote> {
    match steps {
        [device, name] if name.method == "getName" => match device.method.as_str() {
            "getDevice" => Ok(Value::qstring(&frame.device)),
            "getPreviousDevice" => frame
                .previous
                .as_deref()
                .map(Value::qstring)
                .ok_or_else(|| Remote::missing("Device")),
            other => Err(Remote::unknown_method(FRAME, other)),
        },
        [step] => {
            let flag = |value: bool| no_args(step, FRAME).map(|()| Value::Bool(value));
            match step.method.as_str() {
                "getTime" => no_args(step, FRAME).map(|()| Value::Int(frame.time)),
                "getUserTrafficType" => no_args(step, FRAME).map(|()| Value::Int(ICMP)),
                "getSourceString" => no_args(step, FRAME).map(|()| Value::string(&frame.source)),
                "getDestinationString" => {
                    no_args(step, FRAME).map(|()| Value::string(&frame.destination))
                }
                "isFrameAccepted" => flag(frame.accepted),
                "isFrameDropped"
                | "isFrameBuffered"
                | "isFrameOnTransit"
                | "isFrameCollidedOnLink" => flag(false),
                "getFlowChartNodeCount" => no_args(step, FRAME).map(|()| Value::Int(2)),
                "getDecisionAt" => {
                    let index = int_arg(step, FRAME)?;
                    Ok(Value::string(match index {
                        0 => "FastEthernet0 receives the frame.",
                        _ => "The ICMP process received an Echo Request message.",
                    }))
                }
                other => Err(Remote::unknown_method(FRAME, other)),
            }
        }
        _ => Err(Remote::unknown_method(FRAME, "")),
    }
}

pub(super) fn add_simple_pdu(state: &mut State, step: &Step) -> Result<Value, Remote> {
    check_args(
        step,
        "UserCreatedPDU",
        &[TypeCode::QString, TypeCode::QString],
    )?;
    let source = step.args[0].as_str().unwrap_or_default().to_owned();
    let destination = step.args[1].as_str().unwrap_or_default().to_owned();
    let exists = |name: &str| state.devices.iter().any(|device| device.name == name);
    if !exists(&source) {
        return Ok(Value::Int(NO_SOURCE));
    }
    if !exists(&destination) {
        return Ok(Value::Int(NO_DESTINATION));
    }
    if state.simulation.on {
        let time = i32::try_from(state.simulation.clock).unwrap_or(i32::MAX);
        state.simulation.frames.push(Frame {
            time,
            device: source.clone(),
            previous: None,
            source: source.clone(),
            destination: destination.clone(),
            accepted: false,
        });
        state.simulation.pending.push((source, destination));
    }
    Ok(Value::Int(0))
}
