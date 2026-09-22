use ptmp::{Step, Value};

use super::{
    models::{MODELS, MODULES},
    remote::{Remote, count, int_arg},
};

pub(super) fn handle(steps: &[Step]) -> Result<Value, Remote> {
    let [factory, query, rest @ ..] = steps else {
        return Err(Remote::unknown_method("HardwareFactory", ""));
    };
    let table: Vec<(&str, i32)> = match factory.method.as_str() {
        "devices" => MODELS
            .iter()
            .map(|model| (model.name, model.type_code))
            .collect(),
        "modules" => MODULES.to_vec(),
        other => return Err(Remote::unknown_method("HardwareFactory", other)),
    };
    match (query.method.as_str(), rest) {
        (method, []) if method.ends_with("Count") => Ok(count(table.len())),
        (method, [attribute]) if method.starts_with("getAvailable") => {
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
