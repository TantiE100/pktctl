use std::fmt::Write as _;

use crate::{
    PktError,
    elements::{Element, elements, splice},
};

const ROOT: &str = "PACKETTRACER5";
const DEFAULT_CLUSTER: &str = "1-1";

/// A colour as Packet Tracer stores it: one byte per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    #[must_use]
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }

    fn parse_hex(text: &str) -> Option<Self> {
        let digits = text.trim().strip_prefix('#')?;
        if digits.len() != 6 {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
        Some(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Ellipse,
    Rectangle,
    Line,
}

impl ShapeKind {
    fn section(self) -> &'static str {
        match self {
            Self::Ellipse => "ELLIPSES",
            Self::Rectangle => "RECTANGLES",
            Self::Line => "LINES",
        }
    }

    fn element(self) -> &'static str {
        match self {
            Self::Ellipse => "ELLIPSE",
            Self::Rectangle => "RECTANGLE",
            Self::Line => "LINE",
        }
    }
}

/// A drawing on the logical canvas. Ellipses and rectangles span `start` (top left) to
/// `end` (bottom right); a line runs from `start` to `end`. `outline` is the line colour;
/// `fill` only applies to ellipses and rectangles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub start: (i32, i32),
    pub end: (i32, i32),
    pub outline: Rgb,
    pub fill: Option<Rgb>,
}

/// A shape found in the file, with the uuid Packet Tracer knows it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasShape {
    pub uuid: String,
    pub shape: Shape,
}

/// Adds `shape` to the root cluster of the logical canvas and returns the new XML with
/// the uuid given to the shape.
pub fn add_shape(xml: &str, shape: &Shape) -> Result<(String, String), PktError> {
    let uuid = format!("{{{}}}", uuid::Uuid::new_v4());
    Ok((add_shape_with_id(xml, shape, &uuid)?, uuid))
}

/// Adds `shape` under a uuid chosen by the caller, for example to rebuild a canvas.
pub fn add_shape_with_id(xml: &str, shape: &Shape, uuid: &str) -> Result<String, PktError> {
    let all = elements(xml)?;
    let root = all
        .iter()
        .find(|element| element.path == [ROOT])
        .ok_or_else(|| PktError::NodeNotFound(ROOT.into()))?;
    let cluster = all
        .iter()
        .find(|element| element.path == [ROOT, "CLUSTERS", "ROOTCLUSTER", "CLUSTERID"])
        .map_or(DEFAULT_CLUSTER, |element| element.text(xml).trim());
    let item = shape_xml(shape, uuid, cluster);
    let section_name = shape.kind.section();
    let edit = match all
        .iter()
        .find(|element| element.path == [ROOT, section_name])
    {
        Some(Element {
            inner: Some(inner), ..
        }) => (inner.end..inner.end, format!("{item} ")),
        Some(Element {
            outer, inner: None, ..
        }) => (
            outer.clone(),
            format!("<{section_name}>\n{item} </{section_name}>"),
        ),
        None => {
            let closing = root.inner.clone().map_or(root.outer.end, |inner| inner.end);
            (
                closing..closing,
                format!(" <{section_name}>\n{item} </{section_name}>\n"),
            )
        }
    };
    Ok(splice(xml, vec![edit]))
}

/// The ellipses, rectangles and lines of the logical canvas, in file order.
pub fn shapes(xml: &str) -> Result<Vec<CanvasShape>, PktError> {
    let all = elements(xml)?;
    let mut found = Vec::new();
    for kind in [ShapeKind::Ellipse, ShapeKind::Rectangle, ShapeKind::Line] {
        let items = all.iter().filter(|element| {
            element.path.len() == 3
                && element.path[0] == ROOT
                && element.path[1] == kind.section()
                && element.path[2] == kind.element()
        });
        for item in items {
            found.push(read_shape(xml, &all, item, kind));
        }
    }
    found.sort_by_key(|(start, _)| *start);
    Ok(found.into_iter().map(|(_, shape)| shape).collect())
}

fn read_shape(xml: &str, all: &[Element], item: &Element, kind: ShapeKind) -> (usize, CanvasShape) {
    let child = |name: &str| {
        all.iter()
            .find(|element| element.is_child_of(item) && element.name() == name)
    };
    let number = |name: &str| {
        child(name)
            .and_then(|element| element.text(xml).trim().parse::<i32>().ok())
            .unwrap_or_default()
    };
    let color = child("Color").map_or(
        Rgb {
            red: 0,
            green: 0,
            blue: 0,
        },
        |color| {
            let channel = |name: &str| {
                all.iter()
                    .find(|element| element.is_child_of(color) && element.name() == name)
                    .and_then(|element| element.text(xml).trim().parse::<u8>().ok())
                    .unwrap_or_default()
            };
            Rgb {
                red: channel("Red"),
                green: channel("Green"),
                blue: channel("Blue"),
            }
        },
    );
    let (start, end) = if kind == ShapeKind::Line {
        (
            (number("StartX"), number("StartY")),
            (number("EndX"), number("EndY")),
        )
    } else {
        (
            (number("TopLeftX"), number("TopLeftY")),
            (number("BottomRightX"), number("BottomRightY")),
        )
    };
    let (outline, fill) = match child("Filled") {
        Some(filled) if kind != ShapeKind::Line => {
            let tag = &xml
                [filled.outer.start..filled.inner.clone().map_or(filled.outer.end, |i| i.start)];
            let outline = attribute(tag, "OUTLINECOLOR")
                .and_then(|value| Rgb::parse_hex(&value))
                .unwrap_or(color);
            let filled = filled.text(xml).trim() == "1";
            (outline, filled.then_some(color))
        }
        _ => (color, None),
    };
    let uuid = attribute(
        &xml[item.outer.start..item.inner.clone().map_or(item.outer.end, |i| i.start)],
        "uuid",
    )
    .unwrap_or_default();
    (
        item.outer.start,
        CanvasShape {
            uuid,
            shape: Shape {
                kind,
                start,
                end,
                outline,
                fill,
            },
        },
    )
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(tag[at..at + end].to_owned())
}

fn shape_xml(shape: &Shape, uuid: &str, cluster: &str) -> String {
    let element = shape.kind.element();
    let mut item = format!(" <{element} uuid=\"{uuid}\">\n");
    let (start, end) = if shape.kind == ShapeKind::Line {
        (["StartX", "StartY"], ["EndX", "EndY"])
    } else {
        (["TopLeftX", "TopLeftY"], ["BottomRightX", "BottomRightY"])
    };
    let _ = writeln!(item, "   <{0}>{1}</{0}>", start[0], shape.start.0);
    let _ = writeln!(item, "   <{0}>{1}</{0}>", start[1], shape.start.1);
    let _ = writeln!(item, "   <{0}>{1}</{0}>", end[0], shape.end.0);
    let _ = writeln!(item, "   <{0}>{1}</{0}>", end[1], shape.end.1);
    let color = shape.fill.unwrap_or(shape.outline);
    let _ = writeln!(
        item,
        "   <Color>\n    <Red>{}</Red>\n    <Green>{}</Green>\n    <Blue>{}</Blue>\n   </Color>",
        color.red, color.green, color.blue
    );
    if shape.kind != ShapeKind::Line {
        let _ = writeln!(
            item,
            "   <Filled OUTLINECOLOR=\"{}\" OUTLINED=\"true\">{}</Filled>",
            shape.outline.hex(),
            u8::from(shape.fill.is_some())
        );
    }
    let _ = writeln!(
        item,
        "   <{element}CLUSTERID>{cluster}</{element}CLUSTERID>"
    );
    let _ = writeln!(item, "  </{element}>");
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMBER: Rgb = Rgb {
        red: 242,
        green: 163,
        blue: 58,
    };
    const NAVY: Rgb = Rgb {
        red: 11,
        green: 31,
        blue: 51,
    };

    const SAVED: &str = "<PACKETTRACER5>
 <CLUSTERS>
  <ROOTCLUSTER>
   <CLUSTERID>1-1</CLUSTERID>
  </ROOTCLUSTER>
 </CLUSTERS>
 <LINES>
  <LINE uuid=\"{d3f555bb-8a9d-46b7-8091-f2b01bb31cc1}\">
   <StartX>200</StartX>
   <StartY>300</StartY>
   <EndX>600</EndX>
   <EndY>300</EndY>
   <Color>
    <Red>0</Red>
    <Green>160</Green>
    <Blue>0</Blue>
   </Color>
   <LINECLUSTERID>1-1</LINECLUSTERID>
   <MEM_ADDR>0</MEM_ADDR>
  </LINE>
 </LINES>
 <RECTANGLES/>
 <ELLIPSES>
  <ELLIPSE uuid=\"{2c56e395-4e36-4c56-b9d1-d272e1b17a17}\">
   <TopLeftX>400</TopLeftX>
   <TopLeftY>300</TopLeftY>
   <BottomRightX>470</BottomRightX>
   <BottomRightY>370</BottomRightY>
   <Color>
    <Red>255</Red>
    <Green>0</Green>
    <Blue>0</Blue>
   </Color>
   <Filled OUTLINECOLOR=\"#000000\" OUTLINED=\"true\">0</Filled>
   <ELLIPSECLUSTERID>1-1</ELLIPSECLUSTERID>
   <MEM_ADDR>0</MEM_ADDR>
  </ELLIPSE>
 </ELLIPSES>
 <POLYGONS/>
</PACKETTRACER5>";

    #[test]
    fn reads_the_shapes_packet_tracer_saves() {
        let found = shapes(SAVED).unwrap();
        assert_eq!(found.len(), 2);
        let line = &found[0];
        assert_eq!(line.uuid, "{d3f555bb-8a9d-46b7-8091-f2b01bb31cc1}");
        assert_eq!(line.shape.kind, ShapeKind::Line);
        assert_eq!((line.shape.start, line.shape.end), ((200, 300), (600, 300)));
        assert_eq!(line.shape.outline.hex(), "#00A000");
        let ellipse = &found[1];
        assert_eq!(ellipse.shape.kind, ShapeKind::Ellipse);
        assert_eq!(ellipse.shape.end, (470, 370));
        assert_eq!(ellipse.shape.outline.hex(), "#000000");
        assert_eq!(ellipse.shape.fill, None);
    }

    #[test]
    fn adds_an_outlined_circle_to_an_existing_section() {
        let circle = Shape {
            kind: ShapeKind::Ellipse,
            start: (600, 200),
            end: (800, 400),
            outline: AMBER,
            fill: None,
        };
        let (xml, uuid) = add_shape(SAVED, &circle).unwrap();
        assert!(xml.contains("<Filled OUTLINECOLOR=\"#F2A33A\" OUTLINED=\"true\">0</Filled>"));
        let added = shapes(&xml)
            .unwrap()
            .into_iter()
            .find(|found| found.uuid == uuid)
            .unwrap();
        assert_eq!(added.shape, circle);
        assert_eq!(shapes(&xml).unwrap().len(), 3);
    }

    #[test]
    fn opens_an_empty_section_for_a_filled_rectangle() {
        let rectangle = Shape {
            kind: ShapeKind::Rectangle,
            start: (150, 450),
            end: (450, 600),
            outline: NAVY,
            fill: Some(AMBER),
        };
        let (xml, uuid) = add_shape(SAVED, &rectangle).unwrap();
        assert!(!xml.contains("<RECTANGLES/>"));
        assert!(xml.contains("<RECTANGLECLUSTERID>1-1</RECTANGLECLUSTERID>"));
        let added = shapes(&xml)
            .unwrap()
            .into_iter()
            .find(|found| found.uuid == uuid)
            .unwrap();
        assert_eq!(added.shape, rectangle);
    }

    #[test]
    fn creates_a_missing_section_before_the_root_closes() {
        let line = Shape {
            kind: ShapeKind::Line,
            start: (0, 0),
            end: (10, 10),
            outline: AMBER,
            fill: None,
        };
        let (xml, _) = add_shape("<PACKETTRACER5><NETWORK/></PACKETTRACER5>", &line).unwrap();
        assert!(xml.ends_with("</LINES>\n</PACKETTRACER5>"));
        assert_eq!(shapes(&xml).unwrap()[0].shape, line);
    }
}
