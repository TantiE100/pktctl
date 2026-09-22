//! Reads the event registries: the name each class answers to on the wire and
//! the events its `processEvent` accepts.

use std::collections::BTreeMap;

use crate::classfile::Class;
use crate::code::{Insn, walk};

/// Every event class with its events, by the name used on the wire.
pub fn read(classes: &BTreeMap<String, Class>) -> BTreeMap<String, Vec<String>> {
    let mut catalog = BTreeMap::new();
    let registries = classes.iter().filter(|(name, _)| {
        name.starts_with("com.cisco.pt.ipc.events.") && name.ends_with("EventRegistry")
    });
    for (_, class) in registries {
        let mut wire_class = None;
        let mut events: Vec<String> = Vec::new();
        for method in &class.methods {
            let Some(code) = method.code.as_deref() else {
                continue;
            };
            if !method.public {
                continue;
            }
            let insns = walk(code, &class.pool);
            if method.name == "getClassName" && method.descriptor.starts_with("()") {
                wire_class = insns.iter().find_map(|insn| match insn {
                    Insn::Text(text) => Some(text.clone()),
                    _ => None,
                });
            }
            if method.name == "processEvent" {
                for (at, insn) in insns.iter().enumerate() {
                    let Insn::Text(name) = insn else { continue };
                    let compared = insns[at + 1..].iter().take(2).any(|follow| match follow {
                        Insn::Call(member) | Insn::Special(member) => {
                            member.name == "equalsIgnoreCase"
                        }
                        _ => false,
                    });
                    if compared && !events.contains(name) {
                        events.push(name.clone());
                    }
                }
            }
        }
        if let Some(wire_class) = wire_class
            && !events.is_empty()
        {
            catalog.insert(wire_class, events);
        }
    }
    catalog
}
