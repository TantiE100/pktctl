use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use serde::Deserialize;

const INDEX: &str = include_str!("../../assets/ipc-index.json");
const OBJECT_PREFIX: &str = "object:";
const ENUM_PREFIX: &str = "enum:";
const LIST_PREFIX: &str = "list<";

#[derive(Debug, Deserialize)]
pub struct ApiIndex {
    pub classes: BTreeMap<String, ClassDef>,
    pub enums: BTreeMap<String, BTreeMap<String, i64>>,
    pub roots: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct ClassDef {
    pub extends: Vec<String>,
    pub methods: Vec<MethodDef>,
    pub remote: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MethodDef {
    pub name: String,
    #[serde(default)]
    pub ipc: Option<String>,
    pub params: Vec<String>,
    pub returns: String,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default)]
    pub local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind<'a> {
    Bool,
    Byte,
    Bytes,
    Short,
    Int,
    Long,
    Float,
    Double,
    String,
    QString,
    Ip,
    Ipv6,
    Mac,
    Uuid,
    Enum(&'a str),
    Object(&'a str),
    List(&'a str),
    Void,
    Other(&'a str),
}

impl<'a> Kind<'a> {
    pub fn parse(text: &'a str) -> Self {
        if let Some(name) = text.strip_prefix(ENUM_PREFIX) {
            return Self::Enum(name);
        }
        if let Some(name) = text.strip_prefix(OBJECT_PREFIX) {
            return Self::Object(name);
        }
        if let Some(inner) = text
            .strip_prefix(LIST_PREFIX)
            .and_then(|rest| rest.strip_suffix('>'))
        {
            return Self::List(inner);
        }
        match text {
            "bool" => Self::Bool,
            "byte" => Self::Byte,
            "bytes" => Self::Bytes,
            "short" => Self::Short,
            "int" => Self::Int,
            "long" => Self::Long,
            "float" => Self::Float,
            "double" => Self::Double,
            "string" => Self::String,
            "qstring" => Self::QString,
            "ip" => Self::Ip,
            "ipv6" => Self::Ipv6,
            "mac" => Self::Mac,
            "uuid" => Self::Uuid,
            "void" => Self::Void,
            other => Self::Other(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Bool => "bool".into(),
            Self::Byte => "byte".into(),
            Self::Bytes => "bytes".into(),
            Self::Short => "short".into(),
            Self::Int => "int".into(),
            Self::Long => "long".into(),
            Self::Float => "float".into(),
            Self::Double => "double".into(),
            Self::String | Self::QString => "string".into(),
            Self::Ip => "ipv4".into(),
            Self::Ipv6 => "ipv6".into(),
            Self::Mac => "mac".into(),
            Self::Uuid => "uuid".into(),
            Self::Void => "void".into(),
            Self::Enum(name) | Self::Object(name) => name.into(),
            Self::List(inner) => format!("list<{}>", Kind::parse(inner).label()),
            Self::Other(raw) => raw.into(),
        }
    }
}

impl MethodDef {
    pub fn wire_name(&self) -> &str {
        self.ipc.as_deref().unwrap_or(&self.name)
    }

    pub fn param_kinds(&self) -> impl Iterator<Item = Kind<'_>> {
        self.params.iter().map(|param| Kind::parse(param))
    }

    pub fn returns(&self) -> Kind<'_> {
        Kind::parse(&self.returns)
    }

    pub fn param_name(&self, index: usize) -> String {
        self.names
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("arg{index}"))
    }

    pub fn signature(&self) -> String {
        let params: Vec<String> = self
            .param_kinds()
            .enumerate()
            .map(|(index, kind)| format!("{}: {}", self.param_name(index), kind.label()))
            .collect();
        format!(
            "{}({}) -> {}",
            self.name,
            params.join(", "),
            self.returns().label()
        )
    }
}

impl ApiIndex {
    pub(crate) fn get() -> &'static Self {
        static INDEX_CELL: OnceLock<ApiIndex> = OnceLock::new();
        INDEX_CELL.get_or_init(|| {
            serde_json::from_str(INDEX).expect("the embedded IPC index is valid JSON")
        })
    }

    pub fn class(&self, name: &str) -> Option<&ClassDef> {
        self.classes.get(name)
    }

    pub fn ancestors(&self, name: &str) -> Vec<&str> {
        let mut order = Vec::new();
        let mut pending = vec![name];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            if let Some((key, class)) = self.classes.get_key_value(current) {
                order.push(key.as_str());
                pending.extend(class.extends.iter().rev().map(String::as_str));
            }
        }
        order
    }

    pub fn descendants(&self, name: &str) -> Vec<&str> {
        self.classes
            .keys()
            .map(String::as_str)
            .filter(|candidate| *candidate != name && self.ancestors(candidate).contains(&name))
            .collect()
    }

    pub fn methods_named<'s>(&'s self, class: &str, method: &str) -> Vec<(&'s str, &'s MethodDef)> {
        self.ancestors(class)
            .into_iter()
            .flat_map(|owner| {
                self.classes[owner]
                    .methods
                    .iter()
                    .filter(move |candidate| candidate.name == method && !candidate.local)
                    .map(move |candidate| (owner, candidate))
            })
            .collect()
    }

    pub fn enum_values(&self, name: &str) -> Option<&BTreeMap<String, i64>> {
        self.enums.get(name)
    }

    pub fn similar_methods(&self, class: &str, wanted: &str) -> Vec<String> {
        let wanted = wanted.to_lowercase();
        let mut similar: Vec<String> = self
            .ancestors(class)
            .into_iter()
            .flat_map(|owner| self.classes[owner].methods.iter())
            .filter(|method| !method.local)
            .map(|method| method.name.clone())
            .filter(|name| {
                let lower = name.to_lowercase();
                lower.contains(&wanted) || edit_distance(&lower, &wanted) <= 2
            })
            .collect();
        similar.sort();
        similar.dedup();
        similar
    }
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (row, left) in a.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right) in b.iter().enumerate() {
            let substitution = previous[column] + usize::from(left != *right);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_embedded_index_with_verified_signatures() {
        let api = ApiIndex::get();
        assert_eq!(api.roots["network"], "Network");
        let create_link = &api.methods_named("LogicalWorkspace", "createLink")[0].1;
        assert_eq!(
            create_link.params,
            ["qstring", "string", "qstring", "string", "enum:ConnectType"]
        );
        assert_eq!(api.enums["ConnectType"]["ETHERNET_STRAIGHT"], 8100);
        let get_device = &api.methods_named("Network", "getDevice")[0].1;
        assert_eq!(get_device.params, ["qstring"]);
        assert_eq!(get_device.returns(), Kind::Object("Device"));
    }

    #[test]
    fn walks_the_interface_hierarchy_both_ways() {
        let api = ApiIndex::get();
        assert!(api.ancestors("Router").contains(&"Device"));
        assert!(api.descendants("Device").contains(&"Router"));
        assert!(!api.methods_named("Router", "setName").is_empty());
    }

    #[test]
    fn suggests_methods_close_to_a_typo() {
        let api = ApiIndex::get();
        assert!(
            api.similar_methods("Network", "getDevise")
                .contains(&"getDevice".to_owned())
        );
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn formats_signatures_with_javadoc_names() {
        let api = ApiIndex::get();
        let add_module = &api.methods_named("Device", "addModule")[0].1;
        assert_eq!(
            add_module.signature(),
            "addModule(slot: string, type: ModuleType, model: string) -> bool"
        );
    }

    #[test]
    fn every_remote_parameter_has_a_wire_type() {
        let api = ApiIndex::get();
        for (class, def) in api.classes.iter().filter(|(_, def)| def.remote) {
            for method in &def.methods {
                for kind in method.param_kinds() {
                    assert!(
                        !matches!(kind, Kind::Other(_) | Kind::Object(_) | Kind::List(_)),
                        "{class}.{} has an unencodable parameter {kind:?}",
                        method.name
                    );
                }
            }
        }
    }
}
