//! Reads the wire value of every enum constant from the class initialiser,
//! because those values are not the ordinals.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::classfile::Class;
use crate::code::{Insn, walk};
use crate::types::short;

/// Every enum with its constants and the values Packet Tracer sends for them.
pub fn read(classes: &BTreeMap<String, Class>, names: &[String]) -> Map<String, Value> {
    let mut enums: BTreeMap<String, Vec<(String, i64)>> = names
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    for (java_name, class) in classes {
        let name = short(java_name);
        let Some(values) = enums.get_mut(name) else {
            continue;
        };
        let Some(code) = class
            .methods
            .iter()
            .find(|method| method.name == "<clinit>")
            .and_then(|method| method.code.as_deref())
        else {
            continue;
        };
        let insns = walk(code, &class.pool);
        for (at, insn) in insns.iter().enumerate() {
            let Insn::Text(constant) = insn else { continue };
            let mut pushes = Vec::new();
            for follow in insns[at + 1..].iter().take(5) {
                if let Insn::Int(value) = follow {
                    pushes.push(*value);
                }
                if matches!(follow, Insn::Special(_)) {
                    break;
                }
            }
            let value = match pushes.len() {
                0 => continue,
                1 => pushes[0],
                _ => pushes[1],
            };
            match values.iter_mut().find(|(name, _)| name == constant) {
                Some(entry) => entry.1 = value,
                None => values.push((constant.clone(), value)),
            }
        }
    }
    enums
        .into_iter()
        .map(|(name, values)| {
            let values: Map<String, Value> = values
                .into_iter()
                .map(|(constant, value)| (constant, Value::from(value)))
                .collect();
            (name, Value::Object(values))
        })
        .collect()
}
