//! Reads, from the bytecode of the `*Impl` classes and `IPCFactory`, the name
//! each method sends on the wire and the PTMP type of every argument.

use std::collections::HashMap;

use crate::classfile::Class;
use crate::code::{Insn, walk};
use crate::types::{self, short};

/// The `IPCCall.add*Parameter` calls, and the PTMP type each one writes.
const ENCODERS: &[(&str, &str)] = &[
    ("addBoolParameter", "bool"),
    ("addByteParameter", "byte"),
    ("addByteListParameter", "bytes"),
    ("addDoubleParameter", "double"),
    ("addFloatParameter", "float"),
    ("addIntParameter", "int"),
    ("addIPAddressParameter", "ip"),
    ("addIPV6AddressParameter", "ipv6"),
    ("addLongParameter", "long"),
    ("addMACAddressParameter", "mac"),
    ("addQStringParameter", "qstring"),
    ("addShortParameter", "short"),
    ("addStringParameter", "string"),
    ("addUUIDParameter", "uuid"),
];

const FACTORY: &str = "com.cisco.pt.ipc.IPCFactory";

/// What one method sends: the wire name and the type of each argument.
pub type Sent = (String, Vec<String>);

/// Every method of an interface that the implementations send, by interface and
/// method name; a method can appear more than once when it is overloaded.
pub type Wire = HashMap<String, HashMap<String, Vec<Sent>>>;

/// A method of a class, with its instructions.
struct Body<'a> {
    name: &'a str,
    /// The parameter types as the index records them.
    params: Vec<String>,
    insns: Vec<Insn>,
}

fn bodies(class: &Class) -> Vec<Body<'_>> {
    class
        .methods
        .iter()
        .filter(|method| method.public && !method.name.starts_with('<'))
        .map(|method| Body {
            name: &method.name,
            params: types::method(&method.descriptor, method.signature.as_deref()).0,
            insns: method
                .code
                .as_deref()
                .map(|code| walk(code, &class.pool))
                .unwrap_or_default(),
        })
        .collect()
}

/// The PTMP type of each argument a method encodes, in order.
fn encoders(insns: &[Insn]) -> Vec<String> {
    let mut params = Vec::new();
    let mut enum_argument: Option<String> = None;
    for insn in insns {
        let (Insn::Call(member) | Insn::Special(member)) = insn else {
            continue;
        };
        if let Some(rest) = member.owner.strip_prefix("com.cisco.pt.ipc.enums.")
            && !rest.contains('.')
            && member.name.starts_with("get")
            && member.name.ends_with("Value")
        {
            enum_argument = Some(rest.to_owned());
        }
        if short(&member.owner) == "IPCCall"
            && let Some((_, kind)) = ENCODERS.iter().find(|(name, _)| *name == member.name)
        {
            params.push(match enum_argument.take() {
                Some(name) if *kind == "int" => format!("enum:{name}"),
                _ => (*kind).to_owned(),
            });
        }
    }
    params
}

/// Marks as enums the integer arguments whose Java type is one.
fn with_enums(params: &[String], java: &[String]) -> Vec<String> {
    params
        .iter()
        .zip(java)
        .map(|(kind, java)| {
            if kind == "int" && java.contains(".enums.") {
                format!("enum:{}", short(java))
            } else {
                kind.clone()
            }
        })
        .collect()
}

/// `IPCFactory` builds the messages for methods that return objects, so what
/// those methods send has to be read there.
fn factory(class: &Class) -> HashMap<(String, Vec<String>), Sent> {
    let bodies = bodies(class);
    let mut builders: HashMap<(String, Vec<String>), Vec<String>> = HashMap::new();
    for body in &bodies {
        if body.name.starts_with("create") && body.name.ends_with("Message") {
            builders.insert(
                (body.name.to_owned(), body.params.clone()),
                encoders(&body.insns),
            );
        }
    }
    let mut factory = HashMap::new();
    for body in &bodies {
        if builders.contains_key(&(body.name.to_owned(), body.params.clone())) {
            continue;
        }
        let mut name = None;
        let mut used = None;
        for insn in &body.insns {
            match insn {
                Insn::Text(text) if name.is_none() => name = Some(text.clone()),
                Insn::Call(member) | Insn::Special(member)
                    if member.name.starts_with("create") && member.name.ends_with("Message") =>
                {
                    used = builders
                        .get(&(member.name.clone(), types::parameters(&member.descriptor)))
                        .cloned();
                }
                _ => {}
            }
        }
        if let (Some(name), Some(used)) = (name, used) {
            let java = body.params.get(1..).unwrap_or_default();
            factory.insert(
                (body.name.to_owned(), body.params.clone()),
                (name, with_enums(&used, java)),
            );
        }
    }
    factory
}

fn mentions_call(insns: &[Insn]) -> bool {
    insns.iter().any(|insn| match insn {
        Insn::Call(member) | Insn::Special(member) => member.owner.contains("IPCCall"),
        Insn::New(class) => class.contains("IPCCall"),
        _ => false,
    })
}

/// Reads what every implementation sends, either directly or through the factory.
pub fn read(
    classes: &std::collections::BTreeMap<String, Class>,
    impls: &std::collections::BTreeMap<String, Vec<String>>,
) -> Wire {
    let factory = classes.get(FACTORY).map(factory).unwrap_or_default();
    let mut wire = Wire::new();
    for (java_name, targets) in impls {
        let Some(class) = classes.get(java_name) else {
            continue;
        };
        for body in bodies(class) {
            let delegated = body.insns.iter().find_map(|insn| match insn {
                Insn::Call(member) | Insn::Special(member) if member.owner == FACTORY => {
                    Some(member.clone())
                }
                _ => None,
            });
            let mut found = delegated.and_then(|member| {
                factory
                    .get(&(member.name.clone(), types::parameters(&member.descriptor)))
                    .cloned()
            });
            if found.is_none() {
                let name = body.insns.iter().find_map(|insn| match insn {
                    Insn::Text(text) => Some(text.clone()),
                    _ => None,
                });
                match name {
                    Some(name) if mentions_call(&body.insns) => {
                        found = Some((name, encoders(&body.insns)));
                    }
                    _ => continue,
                }
            }
            let Some(found) = found else { continue };
            for target in targets {
                wire.entry(target.clone())
                    .or_default()
                    .entry(body.name.to_owned())
                    .or_default()
                    .push(found.clone());
            }
        }
    }
    wire
}
