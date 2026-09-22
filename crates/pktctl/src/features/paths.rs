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

pub fn logical_workspace() -> Call {
    Call::root("appWindow")
        .method("getActiveWorkspace", [])
        .method("getLogicalWorkspace", [])
}
