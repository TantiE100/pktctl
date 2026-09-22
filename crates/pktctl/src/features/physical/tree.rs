use std::collections::HashMap;

use futures::future::BoxFuture;
use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    features::paths::active_workspace,
    packet_tracer::{PacketTracer, PtError, api::ApiIndex, expect_integer, expect_text},
};

pub(crate) const ROOT_NAME: &str = "Intercity";
const OBJECT_ROOT: &str = "getObjectByUuid";
const KIND_ENUM: &str = "PhysicalObjectType";
const DEVICE_KIND: &str = "device";
const SEPARATOR: char = '/';
const DUPLICATE_MARK: char = '#';

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Location {
    /// Path from Intercity, for example `Home City/Corporate Office`. Empty for Intercity itself.
    pub path: String,
    pub name: String,
    /// `city`, `building`, `wiring_closet`, `rack`, `shelf`, `generic_container`, ...
    pub kind: String,
    pub x: i64,
    pub y: i64,
    /// Devices placed directly inside, by their device name.
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LocationList {
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub uuid: String,
    pub persistent: String,
    pub name: String,
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub path: String,
    pub parent: Option<String>,
    pub device: Option<String>,
}

impl Node {
    pub(crate) fn is_device(&self) -> bool {
        self.kind == DEVICE_KIND
    }
}

pub(crate) fn object(uuid: &str) -> Call {
    Call::root_with(OBJECT_ROOT, [Value::string(uuid)])
}

pub(crate) fn root() -> Call {
    active_workspace().method("getRootPhysicalObject", [])
}

pub(crate) fn split(path: &str) -> Vec<&str> {
    path.split(SEPARATOR)
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

pub(crate) fn normalize(path: &str) -> String {
    let segments = split(path);
    let segments = match segments.split_first() {
        Some((first, rest)) if first.eq_ignore_ascii_case(ROOT_NAME) => rest.to_vec(),
        _ => segments,
    };
    segments.join(&SEPARATOR.to_string())
}

pub(crate) fn plain_name(segment: &str) -> &str {
    segment
        .rsplit_once(DUPLICATE_MARK)
        .filter(|(_, index)| index.parse::<usize>().is_ok())
        .map_or(segment, |(name, _)| name)
}

pub(crate) fn is_duplicate(segment: &str) -> bool {
    plain_name(segment) != segment
}

pub(crate) struct Snapshot {
    pub nodes: Vec<Node>,
}

impl Snapshot {
    pub(crate) async fn read<P: PacketTracer>(packet_tracer: &P) -> Result<Self, PtError> {
        let mut nodes = Vec::new();
        read_node(packet_tracer, root(), String::new(), None, &mut nodes).await?;
        Ok(Self { nodes })
    }

    pub(crate) fn by_persistent(&self, persistent: &str) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|node| node.persistent.eq_ignore_ascii_case(persistent))
    }

    pub(crate) fn location_of(&self, node: &Node) -> Result<Location, PtError> {
        self.location(&node.uuid)
    }

    pub(crate) fn by_uuid(&self, uuid: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.uuid == uuid)
    }

    pub(crate) fn by_path(&self, path: &str) -> Result<&Node, PtError> {
        let wanted = normalize(path);
        self.nodes
            .iter()
            .find(|node| !node.is_device() && node.path == wanted)
            .ok_or_else(|| {
                PtError::NotFound(format!(
                    "location `{path}`; call list_locations to see the paths"
                ))
            })
    }

    pub(crate) fn device(&self, name: &str) -> Result<&Node, PtError> {
        self.nodes
            .iter()
            .find(|node| node.device.as_deref() == Some(name))
            .ok_or_else(|| PtError::NotFound(format!("device `{name}` in the physical workspace")))
    }

    pub(crate) fn children(&self, path: &str) -> impl Iterator<Item = &Node> {
        self.nodes
            .iter()
            .filter(move |node| node.parent.as_deref() == Some(path))
    }

    pub(crate) fn locations(&self) -> LocationList {
        let locations = self
            .nodes
            .iter()
            .filter(|node| !node.is_device())
            .map(|node| Location {
                path: node.path.clone(),
                name: node.name.clone(),
                kind: node.kind.clone(),
                x: node.x,
                y: node.y,
                devices: self
                    .children(&node.path)
                    .filter_map(|child| child.device.clone())
                    .collect(),
            })
            .collect();
        LocationList { locations }
    }

    pub(crate) fn location(&self, uuid: &str) -> Result<Location, PtError> {
        let node = self
            .by_uuid(uuid)
            .ok_or_else(|| PtError::UnexpectedReply(format!("physical object {uuid} vanished")))?;
        self.locations()
            .locations
            .into_iter()
            .find(|location| location.path == node.path)
            .ok_or_else(|| PtError::UnexpectedReply(format!("{} is not a location", node.name)))
    }
}

fn read_node<'a, P: PacketTracer>(
    packet_tracer: &'a P,
    call: Call,
    path: String,
    parent: Option<String>,
    nodes: &'a mut Vec<Node>,
) -> BoxFuture<'a, Result<(), PtError>> {
    Box::pin(async move {
        let get = |method: &str| packet_tracer.call(call.clone().method(method, []));
        let (name, kind, x, y, count, uuid, persistent) = tokio::try_join!(
            get("getName"),
            get("getType"),
            get("getX"),
            get("getY"),
            get("getChildCount"),
            get("getObjectUuid"),
            get("getPathUuid"),
        )?;
        let name = expect_text(&name, "physical object name")?;
        let kind = kind_name(expect_integer(&kind, "physical object type")?);
        let device = if kind == DEVICE_KIND {
            let device_name = packet_tracer
                .call(call.clone().method("getDevice", []).method("getName", []))
                .await;
            Some(device_name.map_or_else(
                |_| name.clone(),
                |value| expect_text(&value, "device name").unwrap_or_else(|_| name.clone()),
            ))
        } else {
            None
        };
        nodes.push(Node {
            uuid: expect_text(&uuid, "physical object uuid")?,
            persistent: expect_text(&persistent, "physical object path uuid")?,
            name,
            kind,
            x: expect_integer(&x, "x")?,
            y: expect_integer(&y, "y")?,
            path: path.clone(),
            parent,
            device,
        });

        let count = expect_integer(&count, "child count")?;
        let mut children = Vec::new();
        for index in 0..count {
            let child = call.clone().method(
                "getChildAt",
                [Value::Int(i32::try_from(index).unwrap_or(i32::MAX))],
            );
            let name = packet_tracer
                .call(child.clone().method("getName", []))
                .await?;
            children.push((child, expect_text(&name, "physical object name")?));
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (child, name) in children {
            let occurrence = seen.entry(name.clone()).or_default();
            *occurrence += 1;
            let segment = if *occurrence == 1 {
                name
            } else {
                format!("{name}{DUPLICATE_MARK}{occurrence}")
            };
            let child_path = if path.is_empty() {
                segment
            } else {
                format!("{path}{SEPARATOR}{segment}")
            };
            read_node(packet_tracer, child, child_path, Some(path.clone()), nodes).await?;
        }
        Ok(())
    })
}

fn kind_name(code: i64) -> String {
    ApiIndex::get()
        .enum_values(KIND_ENUM)
        .and_then(|values| {
            values
                .iter()
                .find(|(_, value)| **value == code)
                .map(|(name, _)| name.to_lowercase())
        })
        .unwrap_or_else(|| format!("type_{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_accept_the_intercity_prefix_and_spacing() {
        assert_eq!(
            normalize("Intercity/Home City/ Corporate Office "),
            "Home City/Corporate Office"
        );
        assert_eq!(normalize("/Home City/"), "Home City");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn duplicate_markers_are_recognized() {
        assert_eq!(plain_name("City#2"), "City");
        assert!(is_duplicate("City#2"));
        assert_eq!(plain_name("Lab #A"), "Lab #A");
        assert!(!is_duplicate("Home City"));
    }

    #[test]
    fn kinds_use_packet_tracer_enum_names() {
        assert_eq!(kind_name(3), "wiring_closet");
        assert_eq!(kind_name(6), "device");
        assert_eq!(kind_name(99), "type_99");
    }
}
