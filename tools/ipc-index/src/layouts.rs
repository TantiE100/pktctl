//! Reads how Packet Tracer writes the value objects it returns (PTMP type 16):
//! each class's wire name, and the field its `read` method takes off the wire.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::classfile::Class;
use crate::code::{Insn, walk};
use crate::types::short;

/// The `read*` calls of a `read` method, and the PTMP type each one takes.
const READERS: &[(&str, &str)] = &[
    ("readBoolean", "bool"),
    ("readByte", "byte"),
    ("readIPCData", "data"),
    ("readDouble", "double"),
    ("readFloat", "float"),
    ("readInt", "int"),
    ("readIPAddress", "ip"),
    ("readIPV6Address", "ipv6"),
    ("readLong", "long"),
    ("readMACAddress", "mac"),
    ("readPair", "pair"),
    ("readShort", "short"),
    ("readQString", "qstring"),
    ("readString", "string"),
    ("readUUID", "uuid"),
    ("readVector", "list"),
];

enum Entry {
    /// The class this one extends, whose fields come first.
    Inherited(String),
    Field { name: Option<String>, kind: String },
}

/// The wire name of each implementation, read from `IPCResponseFactory`: it
/// names the class before it creates it.
fn wire_names(factory: &Class) -> Vec<(String, String)> {
    let mut names = Vec::new();
    let mut pending: Option<String> = None;
    for method in &factory.methods {
        let Some(code) = method.code.as_deref() else {
            continue;
        };
        for insn in walk(code, &factory.pool) {
            match insn {
                Insn::Text(text) => pending = Some(text),
                Insn::New(class) if class.ends_with("Impl") => {
                    if let Some(name) = pending.take() {
                        names.push((class, name));
                    }
                }
                _ => {}
            }
        }
    }
    names
}

/// The fields one `read` method takes off the wire, in order.
fn read_method(insns: &[Insn]) -> (Vec<Entry>, bool) {
    let mut entries = Vec::new();
    let mut variable = false;
    for (at, insn) in insns.iter().enumerate() {
        match insn {
            Insn::Special(member) if member.owner.ends_with("Impl") && member.name == "read" => {
                entries.push(Entry::Inherited(member.owner.clone()));
            }
            Insn::Call(member) | Insn::Special(member) => {
                let Some((_, kind)) = READERS.iter().find(|(name, _)| *name == member.name) else {
                    continue;
                };
                let name = insns[at + 1..]
                    .iter()
                    .take(3)
                    .find_map(|insn| match insn {
                        Insn::PutField(name) => Some(name.clone()),
                        _ => None,
                    });
                entries.push(Entry::Field {
                    name,
                    kind: (*kind).to_owned(),
                });
            }
            Insn::Jump { from, to } if to < from => {
                variable |= insns[at..].iter().any(|later| match later {
                    Insn::Call(member) | Insn::Special(member) => member.name.starts_with("read"),
                    _ => false,
                });
            }
            _ => {}
        }
    }
    (entries, variable)
}

/// Follows a class's `read` up its inheritance, so inherited fields come first.
fn resolve(
    java_name: &str,
    raw: &HashMap<String, Vec<Entry>>,
    variable: &HashSet<String>,
    seen: &mut Vec<String>,
) -> (Vec<Value>, bool) {
    let mut fields = Vec::new();
    let mut loops = variable.contains(java_name);
    for entry in raw.get(java_name).into_iter().flatten() {
        match entry {
            Entry::Inherited(parent) if !seen.contains(parent) => {
                seen.push(java_name.to_owned());
                let (inherited, inherited_loops) = resolve(parent, raw, variable, seen);
                seen.pop();
                fields.extend(inherited);
                loops |= inherited_loops;
            }
            Entry::Inherited(_) => {}
            Entry::Field { name, kind } => {
                fields.push(json!({ "name": name, "kind": kind }));
            }
        }
    }
    (fields, loops)
}

/// The layout of every value object, by the name it uses on the wire.
pub fn read(
    classes: &BTreeMap<String, Class>,
    factory: &Class,
    impls: &BTreeMap<String, Vec<String>>,
) -> Map<String, Value> {
    let mut raw = HashMap::new();
    let mut variable = HashSet::new();
    for (java_name, class) in classes.iter().filter(|(name, _)| name.ends_with("Impl")) {
        for method in &class.methods {
            let Some(code) = method.code.as_deref() else {
                continue;
            };
            if !method.public
                || method.name != "read"
                || crate::types::parameters(&method.descriptor).len() != 1
            {
                continue;
            }
            let (entries, loops) = read_method(&walk(code, &class.pool));
            if loops {
                variable.insert(java_name.clone());
            }
            raw.insert(java_name.clone(), entries);
        }
    }

    let mut layouts = BTreeMap::new();
    for (java_name, wire_name) in wire_names(factory) {
        let (mut fields, loops) = resolve(&java_name, &raw, &variable, &mut Vec::new());
        for (position, field) in fields.iter_mut().enumerate() {
            if field["name"].is_null() {
                field["name"] = Value::String(format!("field{position}"));
            }
        }
        let interface = impls
            .get(&java_name)
            .and_then(|interfaces| interfaces.first().cloned())
            .unwrap_or_else(|| {
                let name = short(&java_name);
                name[..name.len() - "Impl".len()].to_owned()
            });
        let mut layout = Map::new();
        layout.insert("interface".into(), Value::String(interface));
        layout.insert("fields".into(), Value::Array(fields));
        if loops {
            layout.insert("variable".into(), Value::Bool(true));
        }
        layouts.insert(wire_name, Value::Object(layout));
    }
    layouts.into_iter().collect()
}
