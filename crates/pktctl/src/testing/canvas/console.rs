use ptmp::{Event, Step, TypeCode, Value};

use super::{
    State,
    models::INITIAL_DIALOG,
    remote::{Remote, check_args, no_args, string_arg},
};

const CLASS: &str = "TerminalLine";
const INVALID: i32 = 2;
const PAGE_BREAK: &str = "!\n";
const MORE: &str = " --More-- ";
const SPACE: i8 = 32;
const CTRL_SHIFT_6: i8 = 30;
const RELOAD: &str = "reload";
const ENABLE_QUESTION: &str = "Password:";
const ENABLE_SECRET: &str = "enable secret ";
const RELOAD_QUESTION: &str = "Proceed with reload? [confirm]";

pub(super) fn handle(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    let [step] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    match step.method.as_str() {
        "getPrompt" => {
            no_args(step, CLASS)?;
            Ok(Value::string(&state.devices[index].console_prompt))
        }
        "getMode" => {
            no_args(step, CLASS)?;
            Ok(Value::string(state.devices[index].console_mode))
        }
        "getObjectUuid" => {
            no_args(step, CLASS)?;
            Ok(Value::Uuid(terminal_id(&state.devices[index].name)))
        }
        "enterCommand" => {
            let keystroke = string_arg(step, CLASS)?.to_owned();
            type_command(state, index, &keystroke);
            Ok(Value::Void)
        }
        "enterChar" => {
            check_args(step, CLASS, &[TypeCode::Byte, TypeCode::Int])?;
            match step.args[0] {
                Value::Byte(SPACE) => next_page(state, index),
                Value::Byte(CTRL_SHIFT_6) => interrupt(state, index),
                _ => {}
            }
            Ok(Value::Void)
        }
        other => Err(Remote::unknown_method(CLASS, other)),
    }
}

pub(super) fn terminal_id(device: &str) -> String {
    format!("{{console-{device}}}")
}

pub(super) fn prompt(hostname: &str, mode: &str) -> String {
    match mode {
        "user" => format!("{hostname}>"),
        "enable" => format!("{hostname}#"),
        "global" => format!("{hostname}(config)#"),
        _ => format!("{hostname}(config-if)#"),
    }
}

fn type_command(state: &mut State, index: usize, keystroke: &str) {
    let device = &mut state.devices[index];
    let hostname = device.model().hostname;
    match (device.console_prompt.as_str(), keystroke) {
        (INITIAL_DIALOG, "no") => {
            device.console_prompt = String::new();
            return;
        }
        ("", "") => {
            device.console_prompt = prompt(hostname, device.console_mode);
            return;
        }
        (INITIAL_DIALOG | "", _) => return,
        _ => {}
    }

    let terminal = terminal_id(&device.name);
    if device.running.is_some() && answer_pending(state, index, keystroke) {
        return;
    }
    let device = &mut state.devices[index];
    let mode = device.console_mode;
    device.cli.push((mode.to_owned(), keystroke.to_owned()));
    let mut events = vec![written(&terminal, &format!("{keystroke}\n"))];
    let next_mode = match (mode, keystroke) {
        ("user", "enable") | ("global" | "intG", "end") => Some("enable"),
        ("enable", "disable") => Some("user"),
        ("enable", "configure terminal") | ("intG", "exit") => Some("global"),
        ("global" | "intG", command) if command.starts_with("interface ") => Some("intG"),
        _ => None,
    };

    if let Some(secret) = keystroke.strip_prefix(ENABLE_SECRET)
        && mode == "global"
    {
        device.enable_password = Some(secret.to_owned());
        events.push(written(&terminal, "\n"));
        events.push(ended(&terminal, keystroke, 0));
        events.push(written(&terminal, &device.console_prompt));
        state.events.extend(events);
        return;
    }
    if keystroke == "enable" && mode == "user" && device.enable_password.is_some() {
        events.push(written(&terminal, ENABLE_QUESTION));
        device.running = Some(ENABLE_QUESTION.to_owned());
        ENABLE_QUESTION.clone_into(&mut device.console_prompt);
        state.events.extend(events);
        return;
    }
    if keystroke == RELOAD && mode == "enable" {
        events.push(written(&terminal, RELOAD_QUESTION));
        device.running = Some(RELOAD.to_owned());
        RELOAD_QUESTION.clone_into(&mut device.console_prompt);
        state.events.extend(events);
        return;
    }
    if keystroke == "debug hang" {
        events.push(written(&terminal, "waiting forever\n"));
        device.running = Some(keystroke.to_owned());
        state.events.extend(events);
        return;
    }
    if let Some(next) = next_mode {
        device.console_mode = next;
        device.console_prompt = prompt(hostname, next);
        events.push(ended(&terminal, keystroke, 0));
    } else if keystroke.starts_with("bogus") {
        events.push(written(
            &terminal,
            "% Invalid input detected at '^' marker.\n",
        ));
        events.push(ended(&terminal, keystroke, INVALID));
    } else if let Some(target) = keystroke.strip_prefix("ping ") {
        events.push(written(
            &terminal,
            &format!("Sending 5, 100-byte ICMP Echos to {target}, timeout is 2 seconds:\n"),
        ));
        events.extend((0..5).map(|_| written(&terminal, "!")));
        events.push(written(
            &terminal,
            "\nSuccess rate is 100 percent (5/5), round-trip min/avg/max = 0/0/0 ms\n",
        ));
        events.push(ended(&terminal, keystroke, 0));
    } else if keystroke == "show running-config" {
        events.push(written(&terminal, &format!("hostname {hostname}\n")));
        events.push(written(&terminal, PAGE_BREAK));
        events.push(written(&terminal, MORE));
        events.push(terminal_event(
            &terminal,
            "moreDisplayed",
            vec![Value::Int(0)],
        ));
        device.paged = Some(keystroke.to_owned());
    } else {
        events.push(written(&terminal, "\n"));
        events.push(ended(&terminal, keystroke, 0));
    }
    if device.paged.is_none() {
        events.push(written(&terminal, &device.console_prompt));
    }
    state.events.extend(events);
}

/// Answers a question the console left open: the enable password or a reload confirmation.
fn answer_pending(state: &mut State, index: usize, keystroke: &str) -> bool {
    let device = &mut state.devices[index];
    let hostname = device.model().hostname;
    let terminal = terminal_id(&device.name);
    if device.running.as_deref() == Some(ENABLE_QUESTION) {
        device.running = None;
        let accepted = device.enable_password.as_deref() == Some(keystroke);
        if accepted {
            device.console_mode = "enable";
        }
        device.console_prompt = prompt(hostname, device.console_mode);
        let mut events = vec![written(&terminal, "\n")];
        if !accepted {
            events.push(written(&terminal, "% Bad secrets\n"));
        }
        events.push(ended(&terminal, "password", 0));
        events.push(written(&terminal, &device.console_prompt));
        state.events.extend(events);
        return true;
    }
    if device.running.as_deref() == Some(RELOAD) {
        device.running = None;
        let confirmed = keystroke.is_empty() || keystroke.eq_ignore_ascii_case("y");
        if confirmed {
            device.console_mode = "user";
        }
        device.console_prompt = prompt(hostname, device.console_mode);
        state.events.extend([
            written(&terminal, "\n"),
            ended(&terminal, RELOAD, 0),
            written(&terminal, &device.console_prompt),
        ]);
        return true;
    }
    false
}

fn next_page(state: &mut State, index: usize) {
    let device = &mut state.devices[index];
    let terminal = terminal_id(&device.name);
    let Some(command) = device.paged.take() else {
        return;
    };
    state.events.extend([
        written(&terminal, "end\n"),
        ended(&terminal, &command, 0),
        written(&terminal, &device.console_prompt),
    ]);
}

fn interrupt(state: &mut State, index: usize) {
    let device = &mut state.devices[index];
    let terminal = terminal_id(&device.name);
    let Some(command) = device.running.take() else {
        return;
    };
    if command == RELOAD {
        device.console_prompt = prompt(device.model().hostname, device.console_mode);
    }
    state.events.extend([
        written(&terminal, "\n"),
        ended(&terminal, &command, 0),
        written(&terminal, &device.console_prompt),
    ]);
}

fn terminal_event(terminal: &str, name: &str, args: Vec<Value>) -> Event {
    Event {
        token: "canvas".into(),
        class: CLASS.into(),
        object_uuid: terminal.into(),
        name: name.into(),
        args,
    }
}

fn written(terminal: &str, text: &str) -> Event {
    terminal_event(
        terminal,
        "outputWritten",
        vec![Value::string(text), Value::Bool(false), Value::Int(0)],
    )
}

fn ended(terminal: &str, command: &str, status: i32) -> Event {
    terminal_event(
        terminal,
        "commandEnded",
        vec![Value::string(command), Value::Int(status)],
    )
}
