use ptmp::{Call, Value};

pub fn network() -> Call {
    Call::root("network")
}

pub fn device(name: &str) -> Call {
    network().method("getDevice", [Value::qstring(name)])
}

pub fn device_at(index: i32) -> Call {
    network().method("getDeviceAt", [Value::Int(index)])
}

pub fn app_window() -> Call {
    Call::root("appWindow")
}

pub fn active_workspace() -> Call {
    app_window().method("getActiveWorkspace", [])
}

pub fn logical_workspace() -> Call {
    active_workspace().method("getLogicalWorkspace", [])
}

pub fn system_files() -> Call {
    Call::root("systemFileManager")
}
