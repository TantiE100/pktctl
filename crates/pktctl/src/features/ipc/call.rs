use std::net::{Ipv4Addr, Ipv6Addr};

use base64::{Engine, engine::general_purpose::STANDARD};
use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::packet_tracer::{
    PacketTracer, PtError,
    api::{ApiIndex, Kind, MethodDef},
    expect_text,
};

const OBJECT_ROOT: &str = "getObjectByUuid";
const BASE_CLASS: &str = "IPCObject";

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct IpcStep {
    /// Method name as listed by `describe_ipc`, for example `getDevice`.
    pub method: String,
    /// Arguments in order. Strings, numbers and booleans as JSON; IP, MAC and uuid values as
    /// strings; enums by name (`ETHERNET_STRAIGHT`) or number; byte lists as arrays.
    #[serde(default)]
    pub args: Vec<Json>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct IpcCallRequest {
    /// Where the call starts: a root (`network`, `appWindow`, `simulation`, `options`,
    /// `hardwareFactory`, `ipcManager`, `multiUserManager`, `userAppManager`, `commandLog`,
    /// `systemFileManager`) or the uuid of an object returned by an earlier call.
    pub from: String,
    /// Methods to call one after another, each on the object the previous one returned.
    #[serde(default)]
    pub steps: Vec<IpcStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct IpcCallResult {
    /// The call as resolved, for example `network.getDevice("R1").getPower()`.
    pub call: String,
    /// Declared return type of the last method.
    pub returns: String,
    /// The value. Objects come back as `{ "class", "uuid" }`, enums as `{ "name", "value" }`.
    pub value: Json,
}

struct Resolved {
    call: Call,
    rendered: String,
    class: String,
    last: Option<MethodDef>,
}

pub async fn call_ipc<P: PacketTracer>(
    packet_tracer: &P,
    request: &IpcCallRequest,
) -> Result<IpcCallResult, PtError> {
    let api = ApiIndex::get();
    let mut resolved = start(packet_tracer, api, request.from.trim()).await?;
    for step in &request.steps {
        resolved = apply(packet_tracer, api, resolved, step).await?;
    }

    let Some(method) = resolved.last.clone() else {
        return object_value(packet_tracer, resolved, "object").await;
    };
    if let Kind::Object(name) = method.returns()
        && api.is_remote(name)
    {
        let returns = method.returns().label();
        return object_value(packet_tracer, resolved, &returns).await;
    }
    let reply = packet_tracer.call(resolved.call.clone()).await?;
    Ok(IpcCallResult {
        call: resolved.rendered,
        returns: method.returns().label(),
        value: decode(api, method.returns(), reply),
    })
}

async fn start<P: PacketTracer>(
    packet_tracer: &P,
    api: &ApiIndex,
    from: &str,
) -> Result<Resolved, PtError> {
    if let Some(class) = api.roots.get(from) {
        return Ok(Resolved {
            call: Call::root(from),
            rendered: from.to_owned(),
            class: class.clone(),
            last: None,
        });
    }
    if from.starts_with('{') && from.ends_with('}') {
        let call = Call::root_with(OBJECT_ROOT, [Value::string(from)]);
        let class = dynamic_class(packet_tracer, api, &call, BASE_CLASS).await?;
        return Ok(Resolved {
            call,
            rendered: format!("{OBJECT_ROOT}(\"{from}\")"),
            class,
            last: None,
        });
    }
    let roots: Vec<&str> = api.roots.keys().map(String::as_str).collect();
    Err(PtError::InvalidInput(format!(
        "`from` must be a root ({}) or an object uuid such as {{3a0d5999-...}}",
        roots.join(", ")
    )))
}

async fn apply<P: PacketTracer>(
    packet_tracer: &P,
    api: &ApiIndex,
    current: Resolved,
    step: &IpcStep,
) -> Result<Resolved, PtError> {
    if let Some(previous) = &current.last
        && !matches!(previous.returns(), Kind::Object(name) if api.is_remote(name))
    {
        return Err(PtError::InvalidInput(format!(
            "`{}` returns {}, so nothing can be called on it",
            current.rendered,
            previous.returns().label()
        )));
    }

    let mut class = current.class.clone();
    let mut candidates = api.methods_named(&class, &step.method);
    if candidates.is_empty() {
        class = dynamic_class(packet_tracer, api, &current.call, &class).await?;
        candidates = api.methods_named(&class, &step.method);
    }
    if candidates.is_empty() {
        return Err(unknown_method(api, &class, &step.method));
    }

    let mut problems = Vec::new();
    for (_, method) in candidates
        .iter()
        .filter(|(_, method)| method.params.len() == step.args.len())
    {
        match encode_all(api, method, &step.args) {
            Ok(args) => {
                let next_class = match method.returns() {
                    Kind::Object(name) => name.to_owned(),
                    _ => class.clone(),
                };
                return Ok(Resolved {
                    call: current.call.method(method.wire_name(), args),
                    rendered: format!(
                        "{}.{}({})",
                        current.rendered,
                        method.name,
                        render_args(&step.args)
                    ),
                    class: next_class,
                    last: Some((*method).clone()),
                });
            }
            Err(problem) => problems.push(format!("{}: {problem}", method.signature())),
        }
    }
    let signatures: Vec<String> = candidates
        .iter()
        .map(|(owner, method)| format!("{owner}.{}", method.signature()))
        .collect();
    Err(PtError::InvalidInput(if problems.is_empty() {
        format!(
            "`{}` takes a different number of arguments; available: {}",
            step.method,
            signatures.join("; ")
        )
    } else {
        problems.join("; ")
    }))
}

async fn dynamic_class<P: PacketTracer>(
    packet_tracer: &P,
    api: &ApiIndex,
    call: &Call,
    declared: &str,
) -> Result<String, PtError> {
    let reply = packet_tracer
        .call(call.clone().method("getClassName", []))
        .await;
    let actual = match reply {
        Ok(reply) => expect_text(&reply, "class name")?,
        Err(error @ (PtError::Unreachable(_) | PtError::Transport(_))) => return Err(error),
        Err(_) => return Ok(declared.to_owned()),
    };
    Ok(api
        .class_named(&actual)
        .map_or_else(|| declared.to_owned(), str::to_owned))
}

async fn object_value<P: PacketTracer>(
    packet_tracer: &P,
    resolved: Resolved,
    returns: &str,
) -> Result<IpcCallResult, PtError> {
    let (uuid, class) = tokio::try_join!(
        packet_tracer.call(resolved.call.clone().method("getObjectUuid", [])),
        packet_tracer.call(resolved.call.clone().method("getClassName", [])),
    )?;
    Ok(IpcCallResult {
        call: resolved.rendered,
        returns: returns.to_owned(),
        value: json!({
            "class": expect_text(&class, "class name")?,
            "uuid": expect_text(&uuid, "object uuid")?,
        }),
    })
}

fn unknown_method(api: &ApiIndex, class: &str, method: &str) -> PtError {
    let similar = api.similar_methods(class, method);
    let hint = if similar.is_empty() {
        format!("call describe_ipc with class `{class}` to list its methods")
    } else {
        format!("did you mean {}?", similar.join(", "))
    };
    PtError::InvalidInput(format!("`{class}` has no method `{method}`; {hint}"))
}

fn render_args(args: &[Json]) -> String {
    args.iter()
        .map(Json::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn encode_all(api: &ApiIndex, method: &MethodDef, args: &[Json]) -> Result<Vec<Value>, String> {
    method
        .param_kinds()
        .zip(args)
        .enumerate()
        .map(|(index, (kind, arg))| {
            encode(api, kind, arg).map_err(|problem| {
                format!(
                    "argument {} (`{}`) {problem}",
                    index + 1,
                    method.param_name(index)
                )
            })
        })
        .collect()
}

fn encode(api: &ApiIndex, kind: Kind<'_>, arg: &Json) -> Result<Value, String> {
    let integer = |low: i64, high: i64| {
        arg.as_i64()
            .filter(|value| (low..=high).contains(value))
            .ok_or_else(|| format!("must be an integer between {low} and {high}, got {arg}"))
    };
    let text = || {
        arg.as_str()
            .ok_or_else(|| format!("must be a string, got {arg}"))
    };
    Ok(match kind {
        Kind::Bool => Value::Bool(
            arg.as_bool()
                .ok_or_else(|| format!("must be true or false, got {arg}"))?,
        ),
        Kind::Byte => Value::Byte(narrow(integer(i64::from(i8::MIN), i64::from(i8::MAX))?)),
        Kind::Short => Value::Short(narrow(integer(i64::from(i16::MIN), i64::from(i16::MAX))?)),
        Kind::Int => Value::Int(narrow(integer(i64::from(i32::MIN), i64::from(i32::MAX))?)),
        Kind::Long => Value::Long(integer(i64::MIN, i64::MAX)?),
        Kind::Float => Value::Float(to_f32(number(arg)?)),
        Kind::Double => Value::Double(number(arg)?),
        Kind::String => Value::string(text()?),
        Kind::QString => Value::qstring(text()?),
        Kind::Ip => Value::Ip(
            text()?
                .parse::<Ipv4Addr>()
                .map_err(|_| format!("must be an IPv4 address, got {arg}"))?,
        ),
        Kind::Ipv6 => Value::Ipv6(
            text()?
                .parse::<Ipv6Addr>()
                .map_err(|_| format!("must be an IPv6 address, got {arg}"))?,
        ),
        Kind::Mac => Value::Mac(text()?.to_owned()),
        Kind::Uuid => Value::Uuid(text()?.to_owned()),
        Kind::Bytes => Value::Bytes(
            arg.as_array()
                .ok_or_else(|| format!("must be an array of bytes, got {arg}"))?
                .iter()
                .map(|byte| {
                    byte.as_u64()
                        .and_then(|byte| u8::try_from(byte).ok())
                        .ok_or_else(|| format!("must only hold values 0-255, got {byte}"))
                })
                .collect::<Result<_, _>>()?,
        ),
        Kind::Enum(name) => Value::Int(narrow(enum_value(api, name, arg)?)),
        Kind::Object(_) | Kind::List(_) | Kind::Void | Kind::Other(_) => {
            return Err(format!("has type {} which cannot be sent", kind.label()));
        }
    })
}

fn enum_value(api: &ApiIndex, name: &str, arg: &Json) -> Result<i64, String> {
    let values = api
        .enum_values(name)
        .ok_or_else(|| format!("uses unknown enum {name}"))?;
    if let Some(number) = arg.as_i64() {
        return Ok(number);
    }
    let wanted = arg
        .as_str()
        .ok_or_else(|| format!("must be a {name} name or number, got {arg}"))?;
    values
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            let names: Vec<&str> = values.keys().map(String::as_str).collect();
            format!("must be one of {}", names.join(", "))
        })
}

fn number(arg: &Json) -> Result<f64, String> {
    arg.as_f64()
        .ok_or_else(|| format!("must be a number, got {arg}"))
}

#[allow(clippy::cast_possible_truncation)]
fn to_f32(value: f64) -> f32 {
    value as f32
}

fn narrow<T: TryFrom<i64> + Default>(value: i64) -> T {
    T::try_from(value).unwrap_or_default()
}

fn decode(api: &ApiIndex, kind: Kind<'_>, value: Value) -> Json {
    match (kind, value) {
        (Kind::Enum(name), value) => {
            let number = value.as_i64();
            let label = number.and_then(|number| {
                api.enum_values(name)?
                    .iter()
                    .find(|(_, candidate)| **candidate == number)
                    .map(|(label, _)| label.clone())
            });
            json!({ "name": label, "value": number })
        }
        (Kind::List(inner), Value::Vector { items, .. }) => {
            let inner = Kind::parse(inner);
            Json::Array(
                items
                    .into_iter()
                    .map(|item| decode(api, inner, item))
                    .collect(),
            )
        }
        (_, value) => plain(api, value),
    }
}

fn plain(api: &ApiIndex, value: Value) -> Json {
    match value {
        Value::Void => Json::Null,
        Value::Bool(flag) => Json::Bool(flag),
        Value::Byte(number) => json!(number),
        Value::Short(number) => json!(number),
        Value::Int(number) => json!(number),
        Value::Long(number) => json!(number),
        Value::Float(number) => json!(number),
        Value::Double(number) => json!(number),
        Value::String(text) | Value::QString(text) | Value::Mac(text) | Value::Uuid(text) => {
            Json::String(text)
        }
        Value::Ip(address) => Json::String(address.to_string()),
        Value::Ipv6(address) => Json::String(address.to_string()),
        Value::Pair(first, second) => Json::Array(vec![plain(api, *first), plain(api, *second)]),
        Value::Vector { items, .. } => {
            Json::Array(items.into_iter().map(|item| plain(api, item)).collect())
        }
        Value::Bytes(bytes) => json!({ "bytes": bytes.len(), "base64": STANDARD.encode(bytes) }),
        Value::Data { class, fields } => data(api, class, fields),
    }
}

fn data(api: &ApiIndex, class: String, fields: Vec<Value>) -> Json {
    let values: Vec<Json> = fields.into_iter().map(|field| plain(api, field)).collect();
    let mut object = serde_json::Map::new();
    match api.data.get(&class) {
        Some(layout) if !layout.variable && layout.fields.len() == values.len() => {
            object.insert("class".into(), Json::String(layout.interface.clone()));
            for (field, value) in layout.fields.iter().zip(values) {
                object.insert(field.name.clone(), value);
            }
        }
        _ => {
            object.insert("class".into(), Json::String(class));
            object.insert("fields".into(), Json::Array(values));
        }
    }
    Json::Object(object)
}
