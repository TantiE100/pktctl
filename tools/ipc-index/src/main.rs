//! Builds pktctl's IPC index from the Java framework that ships with Packet
//! Tracer, by reading the class files themselves: no JDK, no `javap`.
//!
//! Usage: `ipc-index <framework.jar> <output.json> [<javadoc.zip>] [--with-summaries]`
//!
//! The Javadoc zip only adds the name of each parameter. Its prose belongs to
//! Cisco and is left out unless `--with-summaries` asks for it, which is for a
//! local index, never for one that ships.

mod classfile;
mod code;
mod enums;
mod events;
mod javadoc;
mod layouts;
mod types;
mod wire;

use std::collections::BTreeMap;
use std::io::Read;
use std::process::ExitCode;

use serde_json::{Map, Value};

use classfile::Class;
use types::short;

const IPC_PREFIX: &str = "com/cisco/pt/ipc/";
const RESPONSE_FACTORY: &str = "com/cisco/pt/impl/IPCResponseFactory.class";

type Error = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let summaries = arguments.iter().any(|argument| argument == "--with-summaries");
    let paths: Vec<&str> = arguments
        .iter()
        .filter(|argument| !argument.starts_with("--"))
        .map(String::as_str)
        .collect();
    let [jar, output, javadoc @ ..] = paths.as_slice() else {
        eprintln!("usage: ipc-index <framework.jar> <output.json> [<javadoc.zip>] [--with-summaries]");
        return ExitCode::FAILURE;
    };
    match run(jar, output, javadoc.first().copied(), summaries) {
        Ok(report) => {
            eprintln!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ipc-index: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(jar: &str, output: &str, javadoc: Option<&str>, summaries: bool) -> Result<String, Error> {
    let (classes, factory) = read_jar(jar)?;
    let docs = javadoc::read(javadoc)?;
    let index = build(&classes, factory.as_ref(), &docs, summaries);
    std::fs::write(output, serde_json::to_string(&index)? + "\n")?;
    Ok(report(&index))
}

/// Every IPC class of the jar, by dotted name, plus the response factory.
fn read_jar(path: &str) -> Result<(BTreeMap<String, Class>, Option<Class>), Error> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut classes = BTreeMap::new();
    let mut factory = None;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_owned();
        let response_factory = name == RESPONSE_FACTORY;
        let class_file = std::path::Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("class"));
        let wanted = name.starts_with(IPC_PREFIX) && class_file && !name.contains('$');
        if !wanted && !response_factory {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let class = classfile::read(&bytes).map_err(|error| format!("{name}: {error}"))?;
        if response_factory {
            factory = Some(class);
        } else {
            classes.insert(class.name.clone(), class);
        }
    }
    Ok((classes, factory))
}

/// An interface of the API, as the index records it.
struct Interface {
    extends: Vec<String>,
    methods: Vec<Method>,
}

struct Method {
    name: String,
    /// The Java parameter types, as the framework declares them.
    java: Vec<String>,
    returns: String,
}

/// The classes of the jar, sorted into what the index makes of each kind.
struct Sorted {
    /// The interfaces the index describes, by simple name.
    interfaces: BTreeMap<String, Interface>,
    /// The simple name of every enum, whose wire values are read separately.
    enums: Vec<String>,
    /// Each implementation and the interfaces it implements, by Java name.
    impls: BTreeMap<String, Vec<String>>,
}

/// Sorts the classes of the jar into the interfaces the index describes, the
/// enums whose wire values it needs, and the implementations that carry both.
fn sort_classes(classes: &BTreeMap<String, Class>) -> Sorted {
    let (mut interfaces, mut enums, mut impls) = (BTreeMap::new(), Vec::new(), BTreeMap::new());
    for (java_name, class) in classes {
        let name = short(java_name).to_owned();
        if class.interface && !java_name.contains(".impl.") {
            let methods = class
                .methods
                .iter()
                .filter(|method| method.public && !method.name.starts_with('<'))
                .map(|method| {
                    let (java, returns) =
                        types::method(&method.descriptor, method.signature.as_deref());
                    Method {
                        name: method.name.clone(),
                        java,
                        returns,
                    }
                })
                .collect();
            interfaces.insert(
                name,
                Interface {
                    extends: class.interfaces.iter().map(|parent| short(parent).to_owned()).collect(),
                    methods,
                },
            );
        } else if class.super_name.as_deref() == Some("java.lang.Enum") {
            enums.push(name);
        } else if name.ends_with("Impl") && !class.interfaces.is_empty() {
            impls.insert(
                java_name.clone(),
                class.interfaces.iter().map(|parent| short(parent).to_owned()).collect(),
            );
        }
    }
    Sorted {
        interfaces,
        enums,
        impls,
    }
}

fn build(
    classes: &BTreeMap<String, Class>,
    factory: Option<&Class>,
    docs: &javadoc::Docs,
    summaries: bool,
) -> Value {
    let Sorted {
        interfaces,
        enums: enum_names,
        impls,
    } = sort_classes(classes);
    let enums = enums::read(classes, &enum_names);
    let wire = wire::read(classes, &impls);

    let mut described = Map::new();
    for (name, interface) in &interfaces {
        described.insert(
            name.clone(),
            describe(name, interface, &wire, &interfaces, &enums, docs, summaries),
        );
    }
    mark_remote(&mut described);

    let mut index = Map::new();
    index.insert("classes".into(), Value::Object(described.clone()));
    index.insert("enums".into(), Value::Object(enums.clone()));
    index.insert("roots".into(), Value::Object(roots(&described)));
    index.insert(
        "data".into(),
        Value::Object(factory.map_or_else(Map::new, |factory| {
            layouts::read(classes, factory, &impls)
        })),
    );
    index.insert(
        "events".into(),
        Value::Object(
            events::read(classes)
                .into_iter()
                .map(|(name, events)| (name, Value::from(events)))
                .collect(),
        ),
    );
    Value::Object(index)
}

/// One interface: what it extends and every method with its wire types.
fn describe(
    name: &str,
    interface: &Interface,
    wire: &wire::Wire,
    interfaces: &BTreeMap<String, Interface>,
    enums: &Map<String, Value>,
    docs: &javadoc::Docs,
    summaries: bool,
) -> Value {
    let mut methods = Vec::new();
    for method in &interface.methods {
        let sent = wire
            .get(name)
            .and_then(|methods| methods.get(&method.name))
            .and_then(|sent| {
                sent.iter()
                    .find(|(_, params)| params.len() == method.java.len())
            });
        let mut entry = Map::new();
        entry.insert("name".into(), Value::String(method.name.clone()));
        if let Some((ipc, _)) = sent
            && *ipc != method.name
        {
            entry.insert("ipc".into(), Value::String(ipc.clone()));
        }
        if sent.is_none() {
            entry.insert("local".into(), Value::Bool(true));
        }
        let params = sent.map_or_else(
            || {
                method
                    .java
                    .iter()
                    .map(|java| Value::String(format!("?{}", short(java))))
                    .collect()
            },
            |(_, params)| params.iter().map(|kind| Value::from(kind.clone())).collect(),
        );
        entry.insert("params".into(), Value::Array(params));
        entry.insert(
            "java".into(),
            Value::Array(
                method
                    .java
                    .iter()
                    .map(|java| Value::String(short(java).to_owned()))
                    .collect(),
            ),
        );
        entry.insert(
            "returns".into(),
            Value::String(returns(&method.returns, interfaces, enums)),
        );
        if let Some((names, summary)) =
            docs.get(&(name.to_owned(), method.name.clone(), method.java.len()))
        {
            if names.len() == method.java.len() && !names.is_empty() {
                entry.insert("names".into(), Value::from(names.clone()));
            }
            if summaries && !summary.is_empty() {
                entry.insert("doc".into(), Value::String(summary.clone()));
            }
        }
        methods.push(Value::Object(entry));
    }
    let mut described = Map::new();
    described.insert(
        "extends".into(),
        Value::from(interface.extends.clone()),
    );
    described.insert("methods".into(), Value::Array(methods));
    Value::Object(described)
}

/// A class is remote when it reaches `IPCObject`, which is what makes its
/// methods callable over the wire.
fn mark_remote(classes: &mut Map<String, Value>) {
    let parents: BTreeMap<String, Vec<String>> = classes
        .iter()
        .map(|(name, class)| {
            let extends = class["extends"]
                .as_array()
                .map(|parents| {
                    parents
                        .iter()
                        .filter_map(|parent| parent.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            (name.clone(), extends)
        })
        .collect();
    let names: Vec<String> = classes.keys().cloned().collect();
    for name in names {
        let answer = reaches_ipc_object(&name, &parents, &mut Vec::new());
        if let Some(Value::Object(class)) = classes.get_mut(&name) {
            class.insert("remote".into(), Value::Bool(answer));
        }
    }
}

fn reaches_ipc_object(
    name: &str,
    parents: &BTreeMap<String, Vec<String>>,
    seen: &mut Vec<String>,
) -> bool {
    if name == "IPCObject" {
        return true;
    }
    seen.push(name.to_owned());
    let answer = parents
        .get(name)
        .into_iter()
        .flatten()
        .any(|parent| !seen.contains(parent) && reaches_ipc_object(parent, parents, seen));
    seen.pop();
    answer
}

/// The objects every path can start from: the no-argument methods of `IPC`.
fn roots(classes: &Map<String, Value>) -> Map<String, Value> {
    let mut roots = Map::new();
    let Some(ipc) = classes.get("IPC").and_then(|ipc| ipc["methods"].as_array()) else {
        return roots;
    };
    for method in ipc {
        let returns = method["returns"].as_str().unwrap_or_default();
        if method["params"].as_array().is_some_and(Vec::is_empty)
            && let Some(class) = returns.strip_prefix("object:")
        {
            roots.insert(
                method["name"].as_str().unwrap_or_default().to_owned(),
                Value::String(class.to_owned()),
            );
        }
    }
    roots
}

/// The PTMP type a Java return type becomes.
fn returns(
    java: &str,
    interfaces: &BTreeMap<String, Interface>,
    enums: &Map<String, Value>,
) -> String {
    const JAVA_RETURNS: &[(&str, &str)] = &[
        ("void", "void"),
        ("boolean", "bool"),
        ("java.lang.Boolean", "bool"),
        ("byte", "byte"),
        ("java.lang.Byte", "byte"),
        ("short", "short"),
        ("java.lang.Short", "short"),
        ("int", "int"),
        ("java.lang.Integer", "int"),
        ("long", "long"),
        ("java.lang.Long", "long"),
        ("float", "float"),
        ("double", "double"),
        ("java.lang.String", "string"),
        ("com.cisco.pt.IPAddress", "ip"),
        ("com.cisco.pt.IPV6Address", "ipv6"),
        ("com.cisco.pt.MACAddress", "mac"),
        ("com.cisco.pt.UUID", "uuid"),
    ];
    let base = java.split('<').next().unwrap_or(java);
    if let Some((_, kind)) = JAVA_RETURNS.iter().find(|(name, _)| *name == base) {
        return (*kind).to_owned();
    }
    if matches!(base, "java.util.Vector" | "java.util.List" | "java.util.ArrayList") {
        let inner = java
            .find('<')
            .map_or("?", |open| &java[open + 1..java.len() - 1]);
        return format!("list<{}>", returns(inner, interfaces, enums));
    }
    if base == "com.cisco.pt.util.Pair" {
        return "pair".to_owned();
    }
    let name = short(base);
    if enums.contains_key(name) {
        return format!("enum:{name}");
    }
    if interfaces.contains_key(name) {
        return format!("object:{name}");
    }
    format!("java:{base}")
}

fn report(index: &Value) -> String {
    let classes = index["classes"].as_object().unwrap();
    let methods: usize = classes
        .values()
        .map(|class| class["methods"].as_array().map_or(0, Vec::len))
        .sum();
    let remote = classes
        .values()
        .filter(|class| class["remote"] == Value::Bool(true))
        .count();
    let unresolved: usize = classes
        .values()
        .filter(|class| class["remote"] == Value::Bool(true))
        .flat_map(|class| class["methods"].as_array().unwrap())
        .flat_map(|method| method["params"].as_array().unwrap())
        .filter(|param| param.as_str().is_some_and(|kind| kind.starts_with('?')))
        .count();
    let data = index["data"].as_object().unwrap();
    let variable = data
        .values()
        .filter(|layout| layout.get("variable").is_some())
        .count();
    let events = index["events"].as_object().unwrap();
    let event_count: usize = events
        .values()
        .map(|list| list.as_array().map_or(0, Vec::len))
        .sum();
    format!(
        "{} classes, {methods} methods, {} enums, {} roots, {remote} remote classes, \
         {unresolved} unresolved remote params, {} data layouts ({variable} variable), \
         {} event classes with {event_count} events",
        classes.len(),
        index["enums"].as_object().unwrap().len(),
        index["roots"].as_object().unwrap().len(),
        data.len(),
        events.len(),
    )
}
