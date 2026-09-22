use std::ops::Range;

use quick_xml::{Reader, events::Event};

use crate::PktError;

const WORKSPACE: &[u8] = b"PHYSICALWORKSPACE";
const NODE: &[u8] = b"NODE";
const NAME: &[u8] = b"NAME";
const UUID: &[u8] = b"UUID_STR";
const TYPE: &[u8] = b"TYPE";
const X: &[u8] = b"X";
const Y: &[u8] = b"Y";
const CHILDREN: &[u8] = b"CHILDREN";
const BUILDING: i64 = 2;
const DEVICE: i64 = 6;
const FURNITURE_KINDS: &[i64] = &[4, 5, 8, 9, 10, 11];

/// One `<NODE>` of the physical workspace, located by byte ranges in the XML.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalNode {
    pub uuid: String,
    pub name: String,
    pub kind: i64,
    pub depth: usize,
    pub parent: Option<String>,
    pub x: f64,
    pub y: f64,
    element: Range<usize>,
    name_text: Range<usize>,
    x_text: Range<usize>,
    y_text: Range<usize>,
    children: Children,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Children {
    Open {
        close_tag: usize,
        content: Range<usize>,
    },
    Empty(Range<usize>),
    Missing,
}

#[derive(Default)]
struct Frame {
    start: usize,
    uuid: Option<String>,
    name: Option<(String, Range<usize>)>,
    kind: Option<i64>,
    x: Option<Range<usize>>,
    y: Option<Range<usize>>,
    children: Option<Children>,
    children_open: Option<usize>,
    field_open: Option<usize>,
}

pub fn physical_nodes(xml: &str) -> Result<Vec<PhysicalNode>, PktError> {
    let mut reader = Reader::from_str(xml);
    let mut parser = Parser {
        xml,
        ..Parser::default()
    };
    loop {
        let before = position(&reader);
        let event = reader
            .read_event()
            .map_err(|error| PktError::Xml(error.to_string()))?;
        let after = position(&reader);
        match event {
            Event::Start(tag) => parser.start(tag.name().as_ref(), after),
            Event::Empty(tag) => parser.empty(tag.name().as_ref(), before..after),
            Event::End(tag) => parser.end(tag.name().as_ref(), before, after),
            Event::Eof => break,
            _ => {}
        }
    }
    let mut nodes = parser.nodes;
    link_parents(&mut nodes);
    Ok(nodes)
}

fn position(reader: &Reader<&[u8]>) -> usize {
    usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX)
}

#[derive(Default)]
struct Parser<'x> {
    xml: &'x str,
    nodes: Vec<PhysicalNode>,
    frames: Vec<Frame>,
    path: Vec<Vec<u8>>,
    inside_workspace: bool,
}

impl Parser<'_> {
    fn start(&mut self, name: &[u8], after: usize) {
        if name == WORKSPACE {
            self.inside_workspace = true;
        }
        if self.inside_workspace && name == NODE {
            let start = after - self.open_tag_len(after);
            self.frames.push(Frame {
                start,
                ..Frame::default()
            });
        } else if self.inside_workspace
            && direct_child(&self.path)
            && let Some(frame) = self.frames.last_mut()
        {
            if name == CHILDREN {
                frame.children_open = Some(after);
            } else {
                frame.field_open = Some(after);
            }
        }
        self.path.push(name.to_vec());
    }

    fn open_tag_len(&self, after: usize) -> usize {
        self.xml[..after].rfind('<').map_or(0, |open| after - open)
    }

    fn empty(&mut self, name: &[u8], range: Range<usize>) {
        if self.inside_workspace
            && name == CHILDREN
            && direct_child(&self.path)
            && let Some(frame) = self.frames.last_mut()
        {
            frame.children = Some(Children::Empty(range));
        }
    }

    fn end(&mut self, name: &[u8], before: usize, after: usize) {
        self.path.pop();
        if name == WORKSPACE {
            self.inside_workspace = false;
        }
        if !self.inside_workspace {
            return;
        }
        if name == NODE {
            self.close_node(after);
            return;
        }
        if direct_child(&self.path)
            && let Some(frame) = self.frames.last_mut()
        {
            if name == CHILDREN {
                if let Some(open) = frame.children_open.take() {
                    frame.children = Some(Children::Open {
                        close_tag: before,
                        content: open..before,
                    });
                }
            } else if let Some(open) = frame.field_open.take() {
                record_field(frame, name, self.xml, open..before);
            }
        }
    }

    fn close_node(&mut self, after: usize) {
        let frame = self.frames.pop().unwrap_or_default();
        let (Some(uuid), Some((name, name_text)), Some(kind), Some(x), Some(y)) =
            (frame.uuid, frame.name, frame.kind, frame.x, frame.y)
        else {
            return;
        };
        let number =
            |range: &Range<usize>| self.xml[range.clone()].trim().parse().unwrap_or_default();
        self.nodes.push(PhysicalNode {
            x: number(&x),
            y: number(&y),
            uuid,
            name,
            kind,
            depth: self.frames.len(),
            parent: None,
            element: frame.start..after,
            name_text,
            x_text: x,
            y_text: y,
            children: frame.children.unwrap_or(Children::Missing),
        });
    }
}

fn link_parents(nodes: &mut [PhysicalNode]) {
    let parents: Vec<Option<String>> = nodes
        .iter()
        .map(|node| {
            nodes
                .iter()
                .filter(|other| {
                    other.element.start < node.element.start
                        && node.element.end <= other.element.end
                })
                .max_by_key(|other| other.element.start)
                .map(|other| other.uuid.clone())
        })
        .collect();
    for (node, parent) in nodes.iter_mut().zip(parents) {
        node.parent = parent;
    }
}

fn record_field(frame: &mut Frame, field: &[u8], xml: &str, content: Range<usize>) {
    let raw = &xml[content.clone()];
    match field {
        NAME => {
            let decoded = quick_xml::escape::unescape(raw)
                .map_or_else(|_| raw.to_owned(), std::borrow::Cow::into_owned);
            frame.name = Some((decoded, content));
        }
        UUID => frame.uuid = Some(raw.trim().to_owned()),
        TYPE => frame.kind = raw.trim().parse().ok(),
        X => frame.x = Some(content),
        Y => frame.y = Some(content),
        _ => {}
    }
}

fn direct_child(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|last| last == NODE)
}

fn find<'a>(nodes: &'a [PhysicalNode], uuid: &str) -> Result<&'a PhysicalNode, PktError> {
    nodes
        .iter()
        .find(|node| node.uuid.eq_ignore_ascii_case(uuid))
        .ok_or_else(|| PktError::NodeNotFound(uuid.to_owned()))
}

fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

/// Renames the physical node whose `UUID_STR` is `uuid`.
pub fn rename_node(xml: &str, uuid: &str, name: &str) -> Result<String, PktError> {
    let nodes = physical_nodes(xml)?;
    let node = find(&nodes, uuid)?;
    let mut edited = xml.to_owned();
    edited.replace_range(node.name_text.clone(), &escape(name));
    Ok(edited)
}

/// Moves the node `uuid` inside `parent_uuid`, at `position` when one is given.
/// Packet Tracer mounts anything dropped in a wiring closet into its first rack, so
/// furniture and racks other than that one can only be filled this way.
pub fn move_node(
    xml: &str,
    uuid: &str,
    parent_uuid: &str,
    position: Option<(i32, i32)>,
) -> Result<String, PktError> {
    let nodes = physical_nodes(xml)?;
    let node = find(&nodes, uuid)?;
    let parent = find(&nodes, parent_uuid)?;
    if node.element.start <= parent.element.start && parent.element.end <= node.element.end {
        return Err(PktError::NodeInUse(
            node.name.clone(),
            format!("`{}` is inside it", parent.name),
        ));
    }
    let mut edited = xml.to_owned();
    if node.kind == DEVICE {
        retarget_device(&mut edited, &nodes, node, parent)?;
    }
    let nodes = physical_nodes(&edited)?;
    let node = find(&nodes, uuid)?;
    let moved_range = node.element.clone();
    let base = moved_range.start;
    let mut moved = edited[moved_range.clone()].to_owned();
    if let Some((x, y)) = position {
        let mut places = [
            (shift(&node.x_text, base), x.to_string()),
            (shift(&node.y_text, base), y.to_string()),
        ];
        places.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, value) in places {
            moved.replace_range(range, &value);
        }
    }
    edited.replace_range(moved_range, "");
    let parent_uuid = parent.uuid.clone();
    let nodes = physical_nodes(&edited)?;
    let parent = find(&nodes, &parent_uuid)?;
    match &parent.children {
        Children::Open { close_tag, .. } => edited.insert_str(*close_tag, &moved),
        Children::Empty(range) => {
            edited.replace_range(range.clone(), &format!("<CHILDREN>{moved}</CHILDREN>"));
        }
        Children::Missing => {
            return Err(PktError::NodeNotFound(format!("children of {parent_uuid}")));
        }
    }
    Ok(edited)
}

/// Packet Tracer stores each device's physical path three times: the `PHYSICAL` chain, the
/// `PARENT_PATH` up to the room that holds it, and the `CONTAINER_ID` of the furniture it
/// sits on. A file whose chains disagree with the tree is refused as corrupted.
fn retarget_device(
    xml: &mut String,
    nodes: &[PhysicalNode],
    node: &PhysicalNode,
    parent: &PhysicalNode,
) -> Result<(), PktError> {
    let ancestors = ancestry(nodes, parent);
    let (room, container) = if FURNITURE_KINDS.contains(&parent.kind) {
        (
            &ancestors[..ancestors.len() - 1],
            Some(parent.uuid.as_str()),
        )
    } else {
        (&ancestors[..], None)
    };
    let parent_path = room.join(",");
    let container_id = container.unwrap_or(&parent.uuid).to_owned();
    let mut physical = room.to_vec();
    if let Some(container) = container {
        physical.push(container);
    }
    physical.push(&node.uuid);
    let physical = physical.join(",");

    let marker = format!("{}</PHYSICAL>", node.uuid);
    let end = xml
        .find(&marker)
        .ok_or_else(|| PktError::NodeNotFound(format!("physical chain of {}", node.name)))?;
    let start = xml[..end]
        .rfind("<PHYSICAL>")
        .ok_or_else(|| PktError::NodeNotFound(format!("physical chain of {}", node.name)))?;
    let tail = end + marker.len();
    let block = xml[tail..].to_owned();
    let mut edits = vec![(start + "<PHYSICAL>".len()..end + node.uuid.len(), physical)];
    for (tag, value) in [("PARENT_PATH", parent_path), ("CONTAINER_ID", container_id)] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let at = block
            .find(&open)
            .ok_or_else(|| PktError::NodeNotFound(format!("{tag} of {}", node.name)))?;
        let value_start = tail + at + open.len();
        let value_end = tail
            + block[at..]
                .find(&close)
                .ok_or_else(|| PktError::NodeNotFound(format!("{tag} of {}", node.name)))?
            + at;
        edits.push((value_start..value_end, value));
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, value) in edits {
        xml.replace_range(range, &value);
    }
    Ok(())
}

/// The uuids from the root down to `node`, both included.
fn ancestry<'n>(nodes: &'n [PhysicalNode], node: &'n PhysicalNode) -> Vec<&'n str> {
    let mut chain = vec![node.uuid.as_str()];
    let mut current = node;
    while let Some(parent) = current.parent.as_deref() {
        let Some(above) = nodes.iter().find(|node| node.uuid == parent) else {
            break;
        };
        chain.push(above.uuid.as_str());
        current = above;
    }
    chain.reverse();
    chain
}

/// Removes the node `uuid` with everything inside it. Refuses Intercity and any
/// location that still holds a device, since devices also live in the logical topology.
pub fn remove_node(xml: &str, uuid: &str) -> Result<String, PktError> {
    let nodes = physical_nodes(xml)?;
    let node = find(&nodes, uuid)?;
    if node.parent.is_none() {
        return Err(PktError::NodeInUse(
            node.name.clone(),
            "it is the root of the physical workspace".into(),
        ));
    }
    let devices: Vec<&str> = nodes
        .iter()
        .filter(|inner| {
            inner.kind == DEVICE
                && node.element.start <= inner.element.start
                && inner.element.end <= node.element.end
        })
        .map(|inner| inner.name.as_str())
        .collect();
    if !devices.is_empty() {
        return Err(PktError::NodeInUse(
            node.name.clone(),
            format!("it still holds {}", devices.join(", ")),
        ));
    }
    let mut edited = xml.to_owned();
    edited.replace_range(node.element.clone(), "");
    Ok(edited)
}

/// Adds an empty building inside the node `parent_uuid`; returns the new XML and the building's uuid.
pub fn add_building(
    xml: &str,
    parent_uuid: &str,
    name: &str,
    position: (i32, i32),
) -> Result<(String, String), PktError> {
    add_node(xml, parent_uuid, BUILDING, name, position)
}

/// Adds an empty node of `kind` (a building, rack, table, shelf, ...) inside
/// `parent_uuid`; returns the new XML and the node's uuid.
pub fn add_node(
    xml: &str,
    parent_uuid: &str,
    kind: i64,
    name: &str,
    (x, y): (i32, i32),
) -> Result<(String, String), PktError> {
    let nodes = physical_nodes(xml)?;
    let parent = find(&nodes, parent_uuid)?;
    let uuid = format!("{{{}}}", uuid::Uuid::new_v4());
    let node = node_template(kind, &uuid, name, (x, y));
    let mut edited = xml.to_owned();
    match &parent.children {
        Children::Open { close_tag, .. } => edited.insert_str(*close_tag, &node),
        Children::Empty(range) => {
            edited.replace_range(range.clone(), &format!("<CHILDREN>{node}</CHILDREN>"));
        }
        Children::Missing => {
            return Err(PktError::NodeNotFound(format!("children of {parent_uuid}")));
        }
    }
    Ok((edited, uuid))
}

/// A rack, table, shelf, pegboard or container, with the same fields Packet Tracer writes
/// for the ones its own toolbar creates: no size, scale 1 and no background.
fn furniture(kind: i64, uuid: &str, name: &str, (x, y): (i32, i32)) -> String {
    format!(
        "<NODE><X>{x}</X><Y>{y}</Y><TYPE>{kind}</TYPE>\
         <NAME translate=\"true\">{}</NAME><SX>1</SX><SY>1</SY><W>0</W><H>0</H><D>0</D>\
         <PATH isanim=\"false\"></PATH><CHILDREN/><MANUAL_SCALING>false</MANUAL_SCALING>\
         <SCALED_PIXMAP_WIDTH>0</SCALED_PIXMAP_WIDTH>\
         <SCALED_PIXMAP_HEIGHT>0</SCALED_PIXMAP_HEIGHT><INIT_WIDTH>0</INIT_WIDTH>\
         <INIT_HEIGHT>0</INIT_HEIGHT><INIT_DEPTH>0</INIT_DEPTH><INIT_SX>1</INIT_SX>\
         <INIT_SY>1</INIT_SY><INIT_SZ>1</INIT_SZ><BG_TILED>false</BG_TILED>\
         <CUSTOM_IMAGE_WIDTH>-1</CUSTOM_IMAGE_WIDTH>\
         <CUSTOM_IMAGE_HEIGHT>-1</CUSTOM_IMAGE_HEIGHT><SCALE_FACTOR>1</SCALE_FACTOR>\
         <UUID_STR>{uuid}</UUID_STR><SLOT>0</SLOT><SUB_SLOT>0</SUB_SLOT><ICP_CSX>0</ICP_CSX>\
         <ICP_CSY>0</ICP_CSY><USED>0</USED></NODE>",
        escape(name)
    )
}

/// A building, sized and scaled like the ones Packet Tracer's own toolbar adds to a city, on
/// its building backdrop. Packet Tracer fills in the environment once it opens the file.
fn building(uuid: &str, name: &str, (x, y): (i32, i32)) -> String {
    format!(
        "<NODE><X>{x}</X><Y>{y}</Y><TYPE>{BUILDING}</TYPE>\
         <NAME translate=\"true\">{}</NAME><SX>0.0580719</SX><SY>0.058072</SY><W>200</W>\
         <H>125.261</H><D>3.6576</D>\
         <PATH isanim=\"false\">../art/Background/gGeoViewBuilding.png</PATH><CHILDREN></CHILDREN>\
         <MANUAL_SCALING>false</MANUAL_SCALING>\
         <SCALED_PIXMAP_WIDTH>0</SCALED_PIXMAP_WIDTH>\
         <SCALED_PIXMAP_HEIGHT>0</SCALED_PIXMAP_HEIGHT><INIT_WIDTH>20</INIT_WIDTH>\
         <INIT_HEIGHT>20</INIT_HEIGHT><INIT_DEPTH>3.6576</INIT_DEPTH><INIT_SX>0.2</INIT_SX>\
         <INIT_SY>0.2</INIT_SY><INIT_SZ>0.2</INIT_SZ><BG_TILED>false</BG_TILED>\
         <CUSTOM_IMAGE_WIDTH>-1</CUSTOM_IMAGE_WIDTH>\
         <CUSTOM_IMAGE_HEIGHT>-1</CUSTOM_IMAGE_HEIGHT><SCALE_FACTOR>1</SCALE_FACTOR>\
         <UUID_STR>{uuid}</UUID_STR><SLOT>0</SLOT><SUB_SLOT>0</SUB_SLOT><ICP_CSX>0</ICP_CSX>\
         <ICP_CSY>0</ICP_CSY></NODE>",
        escape(name)
    )
}

fn shift(range: &Range<usize>, base: usize) -> Range<usize> {
    range.start - base..range.end - base
}

/// The XML of a new node, written field by field the way Packet Tracer writes its own.
fn node_template(kind: i64, uuid: &str, name: &str, position: (i32, i32)) -> String {
    if kind == BUILDING {
        building(uuid, name, position)
    } else {
        furniture(kind, uuid, name, position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A physical workspace with the shape Packet Tracer starts from: a world holding one
    /// city, one building inside it and one wiring closet inside that.
    fn empty_network() -> String {
        let node = |kind: i64, name: &str, (x, y): (i32, i32), children: &str| {
            format!(
                "<NODE><X>{x}</X><Y>{y}</Y><TYPE>{kind}</TYPE>\
                 <NAME translate=\"true\">{name}</NAME><SX>1</SX><SY>1</SY><W>20</W><H>20</H>\
                 <D>3.6576</D><PATH isanim=\"false\"></PATH><CHILDREN>{children}</CHILDREN>\
                 <UUID_STR>{{{}}}</UUID_STR></NODE>",
                uuid::Uuid::new_v4()
            )
        };
        let closet = node(3, "Main Wiring Closet", (861, 300), "");
        let office = node(BUILDING, "Corporate Office", (100, 100), &closet);
        let city = node(1, "Home City", (200, 200), &office);
        format!(
            "<?xml version=\"1.0\"?><PACKETTRACER5><VERSION>9.0.1.0858</VERSION>\
             <PHYSICALWORKSPACE>{}</PHYSICALWORKSPACE></PACKETTRACER5>",
            node(0, "Intercity", (0, 0), &city)
        )
    }

    #[test]
    fn lists_the_default_physical_tree() {
        let nodes = physical_nodes(&empty_network()).unwrap();
        let names: Vec<(&str, i64, usize)> = nodes
            .iter()
            .map(|node| (node.name.as_str(), node.kind, node.depth))
            .collect();
        assert_eq!(
            names,
            [
                ("Main Wiring Closet", 3, 3),
                ("Corporate Office", 2, 2),
                ("Home City", 1, 1),
                ("Intercity", 0, 0),
            ]
        );
    }

    #[test]
    fn renames_only_the_chosen_node() {
        let xml = empty_network();
        let city = physical_nodes(&xml).unwrap()[2].uuid.clone();
        let renamed = rename_node(&xml, &city, "La Paz & El Alto").unwrap();
        let nodes = physical_nodes(&renamed).unwrap();
        assert_eq!(nodes[2].name, "La Paz & El Alto");
        assert!(renamed.contains("La Paz &amp; El Alto"));
        assert_eq!(nodes[1].name, "Corporate Office");
        assert_eq!(nodes[1].parent.as_deref(), Some(city.as_str()));
        assert_eq!((nodes[2].x, nodes[2].y), (200.0, 200.0));
        assert_eq!(
            renamed.len(),
            xml.len() - "Home City".len() + "La Paz &amp; El Alto".len()
        );
    }

    #[test]
    fn adds_an_empty_building_to_a_city() {
        let xml = empty_network();
        let city = physical_nodes(&xml).unwrap()[2].uuid.clone();
        let (edited, uuid) = add_building(&xml, &city, "Edificio GAMC", (320, 140)).unwrap();
        let nodes = physical_nodes(&edited).unwrap();
        let building = nodes.iter().find(|node| node.uuid == uuid).unwrap();
        assert_eq!(
            (building.name.as_str(), building.kind, building.depth),
            ("Edificio GAMC", 2, 2)
        );
        assert_eq!(&edited[building.x_text.clone()], "320");
        assert!(
            matches!(building.children, Children::Open { ref content, .. } if content.is_empty())
        );
        assert_eq!(nodes.len(), 5);
        assert!(crate::decode(&crate::encode(&edited).unwrap()).is_ok());
    }

    #[test]
    fn removes_a_location_with_its_empty_children() {
        let xml = empty_network();
        let nodes = physical_nodes(&xml).unwrap();
        let office = nodes[1].uuid.clone();
        let edited = remove_node(&xml, &office).unwrap();
        let names: Vec<String> = physical_nodes(&edited)
            .unwrap()
            .into_iter()
            .map(|node| node.name)
            .collect();
        assert_eq!(names, ["Home City", "Intercity"]);
        assert!(crate::decode(&crate::encode(&edited).unwrap()).is_ok());
    }

    #[test]
    fn keeps_intercity_and_locations_holding_devices() {
        let xml = empty_network();
        let nodes = physical_nodes(&xml).unwrap();
        let root = nodes[3].uuid.clone();
        assert!(matches!(
            remove_node(&xml, &root),
            Err(PktError::NodeInUse(name, _)) if name == "Intercity"
        ));

        let city = nodes[2].uuid.clone();
        let (with_building, building) = add_building(&xml, &city, "Anexo", (10, 10)).unwrap();
        let device = "<NODE><X>1</X><Y>1</Y><TYPE>6</TYPE><NAME>R1</NAME><CHILDREN/>\
                      <UUID_STR>{device}</UUID_STR></NODE>";
        let anexo = physical_nodes(&with_building)
            .unwrap()
            .into_iter()
            .find(|node| node.uuid == building)
            .unwrap();
        let Children::Open { close_tag, .. } = anexo.children else {
            panic!("a new building has open children");
        };
        let mut occupied = with_building.clone();
        occupied.insert_str(close_tag, device);
        assert_eq!(
            remove_node(&occupied, &building),
            Err(PktError::NodeInUse(
                "Anexo".into(),
                "it still holds R1".into()
            ))
        );
    }

    #[test]
    fn writes_furniture_the_way_packet_tracer_does() {
        let xml = empty_network();
        let closet = physical_nodes(&xml).unwrap()[0].uuid.clone();
        let (edited, uuid) = add_node(&xml, &closet, 10, "Mesa Técnica", (5, 7)).unwrap();
        let nodes = physical_nodes(&edited).unwrap();
        let table = nodes.iter().find(|node| node.uuid == uuid).unwrap();
        assert_eq!(
            (table.name.as_str(), table.kind, table.parent.as_deref()),
            ("Mesa Técnica", 10, Some(closet.as_str()))
        );
        assert_eq!((table.x, table.y), (5.0, 7.0));
        let element = &edited[edited.find("<TYPE>10</TYPE>").unwrap() - 40..];
        for field in [
            "<SX>1</SX>",
            "<W>0</W>",
            "<INIT_SZ>1</INIT_SZ>",
            "<USED>0</USED>",
        ] {
            assert!(element.contains(field), "furniture keeps {field}");
        }
        assert!(crate::decode(&crate::encode(&edited).unwrap()).is_ok());
    }

    #[test]
    fn moves_a_node_and_its_position() {
        let xml = empty_network();
        let nodes = physical_nodes(&xml).unwrap();
        let (closet, city) = (nodes[0].uuid.clone(), nodes[2].uuid.clone());
        let moved = move_node(&xml, &closet, &city, Some((12, 34))).unwrap();
        let nodes = physical_nodes(&moved).unwrap();
        let closet = nodes.iter().find(|node| node.uuid == closet).unwrap();
        assert_eq!(closet.parent.as_deref(), Some(city.as_str()));
        assert_eq!((closet.x, closet.y), (12.0, 34.0));

        let nodes = physical_nodes(&xml).unwrap();
        let (office, closet) = (nodes[1].uuid.clone(), nodes[0].uuid.clone());
        assert!(matches!(
            move_node(&xml, &office, &closet, None),
            Err(PktError::NodeInUse(..))
        ));
    }

    #[test]
    fn reports_unknown_nodes() {
        assert_eq!(
            rename_node(&empty_network(), "{missing}", "x"),
            Err(PktError::NodeNotFound("{missing}".into()))
        );
    }
}
