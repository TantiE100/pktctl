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
const TEMPLATE_FILE: &[u8] = include_bytes!("../assets/empty-9.0.1.pkt");

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

/// Adds an empty building inside the node `parent_uuid`; returns the new XML and the building's uuid.
pub fn add_building(
    xml: &str,
    parent_uuid: &str,
    name: &str,
    (x, y): (i32, i32),
) -> Result<(String, String), PktError> {
    let nodes = physical_nodes(xml)?;
    let parent = find(&nodes, parent_uuid)?;
    let uuid = format!("{{{}}}", uuid::Uuid::new_v4());
    let building = building_template(&uuid, name, (x, y))?;
    let mut edited = xml.to_owned();
    match &parent.children {
        Children::Open { close_tag, .. } => edited.insert_str(*close_tag, &building),
        Children::Empty(range) => {
            edited.replace_range(range.clone(), &format!("<CHILDREN>{building}</CHILDREN>"));
        }
        Children::Missing => {
            return Err(PktError::NodeNotFound(format!("children of {parent_uuid}")));
        }
    }
    Ok((edited, uuid))
}

fn building_template(uuid: &str, name: &str, (x, y): (i32, i32)) -> Result<String, PktError> {
    let empty = crate::decode(TEMPLATE_FILE)?;
    let nodes = physical_nodes(&empty)?;
    let template = nodes
        .iter()
        .find(|node| node.kind == BUILDING)
        .ok_or_else(|| PktError::NodeNotFound("building template".into()))?;
    let base = template.element.start;
    let mut replacements = vec![
        (shift(&template.name_text, base), escape(name)),
        (shift(&template.x_text, base), x.to_string()),
        (shift(&template.y_text, base), y.to_string()),
    ];
    if let Children::Open { content, .. } = &template.children {
        replacements.push((shift(content, base), String::new()));
    }
    let mut text = empty[template.element.clone()].to_owned();
    let uuid_start = text
        .rfind(&template.uuid)
        .ok_or_else(|| PktError::NodeNotFound("building template uuid".into()))?;
    replacements.push((
        uuid_start..uuid_start + template.uuid.len(),
        uuid.to_owned(),
    ));
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, value) in replacements {
        text.replace_range(range, &value);
    }
    Ok(text)
}

fn shift(range: &Range<usize>, base: usize) -> Range<usize> {
    range.start - base..range.end - base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_network() -> String {
        crate::decode(TEMPLATE_FILE).unwrap()
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
    fn reports_unknown_nodes() {
        assert_eq!(
            rename_node(&empty_network(), "{missing}", "x"),
            Err(PktError::NodeNotFound("{missing}".into()))
        );
    }
}
