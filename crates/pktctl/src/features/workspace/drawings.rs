use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::{
        network_file::edit_saved_network, network_file::file_error, paths::logical_workspace,
    },
    packet_tracer::{PacketTracer, PtError, expect_text},
};

const DEFAULT_RADIUS: i32 = 60;

/// Colours by name, so a drawing can be asked for in words.
const COLOURS: &[(&str, (i32, i32, i32))] = &[
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("gray", (128, 128, 128)),
    ("red", (200, 30, 30)),
    ("orange", (230, 130, 20)),
    ("yellow", (230, 200, 40)),
    ("green", (40, 150, 60)),
    ("blue", (30, 90, 200)),
    ("purple", (120, 60, 180)),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Drawing {
    /// A circle, to ring a subnet or a group of devices.
    #[default]
    Circle,
    /// A rectangle, to frame an area such as a building, a floor or a site.
    Rectangle,
    /// A straight line, to mark a boundary or point at something.
    Line,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DrawRequest {
    /// `circle` (default), `rectangle` or `line`.
    #[serde(default)]
    pub shape: Drawing,
    /// Circle: the centre. Rectangle: one corner. Line: where it starts. Canvas
    /// coordinates, as in `add_device`.
    pub x: i32,
    pub y: i32,
    /// Rectangle: the opposite corner. Line: where it ends.
    #[serde(default)]
    pub to_x: Option<i32>,
    #[serde(default)]
    pub to_y: Option<i32>,
    /// Circle: its radius. Defaults to 60.
    #[serde(default)]
    pub radius: Option<i32>,
    /// Outline colour: `red`, `orange`, `yellow`, `green`, `blue`, `purple`, `gray`,
    /// `black`, `white`, or a `#rrggbb` value. Defaults to blue.
    #[serde(default)]
    pub color: Option<String>,
    /// Circle and rectangle: fill them with this colour, named or `#rrggbb`. Left
    /// unfilled when omitted.
    #[serde(default)]
    pub fill: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Drawn {
    pub id: String,
    pub shape: Drawing,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// The temporary copy Packet Tracer now has open: drawings go through the network
    /// file. Your own file is untouched: save with `save_network` and a path to keep the
    /// drawing there.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DrawingItem {
    pub id: String,
    pub shape: Drawing,
    /// Centre of the drawing on the canvas.
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DrawingList {
    pub drawings: Vec<DrawingItem>,
}

/// Packet Tracer's own `drawCircle` and `drawLine` read their coordinates as window
/// pixels, treat the radius as the diagonal of the bounding box, and paint every
/// circle's outline black whatever colour is given. Drawings are therefore written into
/// the network file, where Packet Tracer stores them exactly.
pub async fn draw<P: PacketTracer>(
    packet_tracer: &P,
    request: &DrawRequest,
) -> Result<Drawn, PtError> {
    let outline = rgb(colour(request.color.as_deref())?);
    let fill = match request.fill.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => {
            if request.shape == Drawing::Line {
                return Err(PtError::InvalidInput(
                    "a line cannot be filled; fill applies to circles and rectangles".into(),
                ));
            }
            Some(rgb(colour(Some(name))?))
        }
        _ => None,
    };
    let (kind, start, end) = geometry(request)?;
    let shape = pktfile::Shape {
        kind,
        start,
        end,
        outline,
        fill,
    };
    let mut id = String::new();
    let opened = edit_saved_network(packet_tracer, |xml| {
        let (edited, uuid) = pktfile::add_shape(xml, &shape).map_err(|error| file_error(&error))?;
        id = uuid;
        Ok(edited)
    })
    .await?;
    Ok(Drawn {
        id,
        shape: request.shape,
        color: outline.hex().to_ascii_lowercase(),
        fill: fill.map(|fill| fill.hex().to_ascii_lowercase()),
        file: opened,
    })
}

type Point = (i32, i32);

fn geometry(request: &DrawRequest) -> Result<(pktfile::ShapeKind, Point, Point), PtError> {
    let corner = || match (request.to_x, request.to_y) {
        (Some(to_x), Some(to_y)) => Ok((to_x, to_y)),
        _ => Err(PtError::InvalidInput(format!(
            "a {} needs to_x and to_y as well",
            match request.shape {
                Drawing::Rectangle => "rectangle",
                _ => "line",
            }
        ))),
    };
    match request.shape {
        Drawing::Circle => {
            let radius = request.radius.unwrap_or(DEFAULT_RADIUS);
            if radius <= 0 {
                return Err(PtError::InvalidInput(format!(
                    "radius must be positive, got {radius}"
                )));
            }
            Ok((
                pktfile::ShapeKind::Ellipse,
                (request.x - radius, request.y - radius),
                (request.x + radius, request.y + radius),
            ))
        }
        Drawing::Rectangle => {
            let (to_x, to_y) = corner()?;
            Ok((
                pktfile::ShapeKind::Rectangle,
                (request.x.min(to_x), request.y.min(to_y)),
                (request.x.max(to_x), request.y.max(to_y)),
            ))
        }
        Drawing::Line => Ok((pktfile::ShapeKind::Line, (request.x, request.y), corner()?)),
    }
}

fn rgb((red, green, blue): (i32, i32, i32)) -> pktfile::Rgb {
    let channel = |value: i32| u8::try_from(value.clamp(0, 255)).unwrap_or(u8::MAX);
    pktfile::Rgb {
        red: channel(red),
        green: channel(green),
        blue: channel(blue),
    }
}

pub async fn list_drawings<P: PacketTracer>(packet_tracer: &P) -> Result<DrawingList, PtError> {
    let mut drawings = Vec::new();
    for (shape, method) in [
        (Drawing::Circle, "getCanvasEllipseIds"),
        (Drawing::Rectangle, "getCanvasRectIds"),
        (Drawing::Line, "getCanvasLineIds"),
    ] {
        let ids = packet_tracer
            .call(logical_workspace().method(method, []))
            .await?;
        let Value::Vector { items, .. } = ids else {
            continue;
        };
        for id in items {
            let id = expect_text(&id, "drawing id")?;
            let (x, y) = position(packet_tracer, &id).await?;
            drawings.push(DrawingItem { id, shape, x, y });
        }
    }
    Ok(DrawingList { drawings })
}

pub async fn remove_drawing<P: PacketTracer>(
    packet_tracer: &P,
    id: &str,
) -> Result<String, PtError> {
    let removed = packet_tracer
        .call(logical_workspace().method("removeCanvasItem", [Value::Uuid(id.trim().to_owned())]))
        .await?;
    if removed.as_bool() == Some(false) {
        return Err(PtError::NotFound(format!("drawing `{id}`")));
    }
    Ok(id.trim().to_owned())
}

async fn position<P: PacketTracer>(packet_tracer: &P, id: &str) -> Result<(i64, i64), PtError> {
    let (x, y) = tokio::try_join!(
        packet_tracer
            .call(logical_workspace().method("getCanvasItemX", [Value::Uuid(id.to_owned())])),
        packet_tracer
            .call(logical_workspace().method("getCanvasItemY", [Value::Uuid(id.to_owned())])),
    )?;
    Ok((
        x.as_i64().unwrap_or_default(),
        y.as_i64().unwrap_or_default(),
    ))
}

fn colour(name: Option<&str>) -> Result<(i32, i32, i32), PtError> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok((30, 90, 200));
    };
    if let Some((_, rgb)) = COLOURS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
    {
        return Ok(*rgb);
    }
    let hex = name.strip_prefix('#').filter(|hex| hex.len() == 6);
    if let Some(hex) = hex
        && let Ok(value) = i32::from_str_radix(hex, 16)
    {
        return Ok(((value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff));
    }
    let names: Vec<&str> = COLOURS.iter().map(|(name, _)| *name).collect();
    Err(PtError::InvalidInput(format!(
        "`{name}` is not a colour; use #rrggbb or one of {}",
        names.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_colours_by_name_and_by_hex() {
        assert_eq!(colour(None).unwrap(), (30, 90, 200));
        assert_eq!(colour(Some(" RED ")).unwrap(), (200, 30, 30));
        assert_eq!(colour(Some("#ff8800")).unwrap(), (255, 136, 0));
        let error = colour(Some("turquesa")).unwrap_err().to_string();
        assert!(error.contains("#rrggbb"), "{error}");
    }
}
