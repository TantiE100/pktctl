use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    features::paths::logical_workspace,
    packet_tracer::{PacketTracer, PtError, expect_text},
};

const DEFAULT_WIDTH: i32 = 2;
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
    /// A straight line, to mark a boundary or point at something.
    Line,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DrawRequest {
    /// `circle` (default) or `line`.
    #[serde(default)]
    pub shape: Drawing,
    /// Circle: the centre. Line: where it starts. Canvas coordinates, as in `add_device`.
    pub x: i32,
    pub y: i32,
    /// Line: where it ends.
    #[serde(default)]
    pub to_x: Option<i32>,
    #[serde(default)]
    pub to_y: Option<i32>,
    /// Circle: how wide it is. Defaults to 60.
    #[serde(default)]
    pub radius: Option<i32>,
    /// Line thickness. Defaults to 2.
    #[serde(default)]
    pub width: Option<i32>,
    /// `red`, `orange`, `yellow`, `green`, `blue`, `purple`, `gray`, `black`, `white`, or
    /// a `#rrggbb` value. Defaults to blue.
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Drawn {
    pub id: String,
    pub shape: Drawing,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DrawingItem {
    pub id: String,
    pub shape: Drawing,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DrawingList {
    pub drawings: Vec<DrawingItem>,
}

pub async fn draw<P: PacketTracer>(
    packet_tracer: &P,
    request: &DrawRequest,
) -> Result<Drawn, PtError> {
    let (red, green, blue) = colour(request.color.as_deref())?;
    let layer = layer(packet_tracer).await?;
    let call = match request.shape {
        Drawing::Circle => logical_workspace().method(
            "drawCircle",
            [
                Value::Int(request.x),
                Value::Int(request.y),
                Value::Double(layer),
                Value::Int(request.radius.unwrap_or(DEFAULT_RADIUS)),
                Value::Int(red),
                Value::Int(green),
                Value::Int(blue),
            ],
        ),
        Drawing::Line => {
            let (Some(to_x), Some(to_y)) = (request.to_x, request.to_y) else {
                return Err(PtError::InvalidInput(
                    "a line needs to_x and to_y as well".into(),
                ));
            };
            logical_workspace().method(
                "drawLine",
                [
                    Value::Int(request.x),
                    Value::Int(request.y),
                    Value::Int(to_x),
                    Value::Int(to_y),
                    Value::Double(layer),
                    Value::Int(request.width.unwrap_or(DEFAULT_WIDTH)),
                    Value::Int(red),
                    Value::Int(green),
                    Value::Int(blue),
                ],
            )
        }
    };
    let id = packet_tracer.call(call).await?;
    Ok(Drawn {
        id: expect_text(&id, "drawing id")?,
        shape: request.shape,
        color: format!("#{red:02x}{green:02x}{blue:02x}"),
    })
}

pub async fn list_drawings<P: PacketTracer>(packet_tracer: &P) -> Result<DrawingList, PtError> {
    let mut drawings = Vec::new();
    for (shape, method) in [
        (Drawing::Circle, "getCanvasEllipseIds"),
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

/// The z order Packet Tracer hands out for the next drawing, so shapes stack in order.
async fn layer<P: PacketTracer>(packet_tracer: &P) -> Result<f64, PtError> {
    let layer = packet_tracer
        .call(logical_workspace().method("getIncNoteZOrder", []))
        .await?;
    match layer {
        Value::Double(layer) => Ok(layer),
        Value::Float(layer) => Ok(f64::from(layer)),
        Value::Int(layer) => Ok(f64::from(layer)),
        other => Err(PtError::UnexpectedReply(format!(
            "the drawing layer should be a number, got {other:?}"
        ))),
    }
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
