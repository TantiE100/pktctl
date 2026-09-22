use ptmp::{Step, TypeCode, Value};

use super::{
    State,
    remote::{Remote, check_args, int_arg, no_args, qstring_arg},
};

const CLASS: &str = "PhysicalObject";
const TOOLBAR: &str = "PhysicalToolbar";
const UNIVERSE: i32 = 0;
const CITY: i32 = 1;
const BUILDING: i32 = 2;
const CLOSET: i32 = 3;
const RACK: i32 = 4;
const DEVICE: i32 = 6;
const HOME_CLOSET: usize = 3;
const OFFICE: usize = 2;

#[derive(Debug, Clone)]
pub(super) struct Place {
    persistent: String,
    name: String,
    kind: i32,
    x: i32,
    y: i32,
    parent: Option<usize>,
    removed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Physical {
    places: Vec<Place>,
    current: usize,
    cities_added: i32,
}

impl Default for Physical {
    fn default() -> Self {
        let place = |name: &str, kind, x, y, parent: Option<usize>| Place {
            persistent: persistent_id(
                parent.map_or(0, |parent| parent + 1) * 100
                    + usize::try_from(kind).unwrap_or_default(),
            ),
            name: name.into(),
            kind,
            x,
            y,
            parent,
            removed: false,
        };
        Self {
            places: vec![
                place("Intercity", UNIVERSE, 0, 0, None),
                place("Home City", CITY, 200, 200, Some(0)),
                place("Corporate Office", BUILDING, 100, 100, Some(1)),
                place("Main Wiring Closet", CLOSET, 861, 300, Some(OFFICE)),
            ],
            current: 0,
            cities_added: 0,
        }
    }
}

impl Physical {
    fn children(&self, id: usize) -> Vec<usize> {
        (0..self.places.len())
            .filter(|child| !self.places[*child].removed && self.places[*child].parent == Some(id))
            .collect()
    }

    fn add(&mut self, name: &str, kind: i32, (x, y): (i32, i32), parent: usize) -> usize {
        self.places.push(Place {
            persistent: persistent_id(1000 + self.places.len()),
            name: name.into(),
            kind,
            x,
            y,
            parent: Some(parent),
            removed: false,
        });
        self.places.len() - 1
    }

    fn rack_in(&mut self, closet: usize) -> usize {
        self.children(closet)
            .into_iter()
            .find(|child| self.places[*child].kind == RACK)
            .unwrap_or_else(|| self.add("Rack", RACK, (0, 0), closet))
    }

    pub(super) fn place_device(&mut self, name: &str, in_rack: bool) {
        let parent = if in_rack {
            self.rack_in(HOME_CLOSET)
        } else {
            OFFICE
        };
        self.add(name, DEVICE, (0, 0), parent);
    }

    pub(super) fn remove_device(&mut self, name: &str) {
        for place in &mut self.places {
            if place.kind == DEVICE && place.name == name {
                place.removed = true;
            }
        }
    }

    pub(super) fn device_place(&self, name: &str) -> Option<usize> {
        (0..self.places.len()).find(|id| {
            let place = &self.places[*id];
            !place.removed && place.kind == DEVICE && place.name == name
        })
    }

    fn relocate(&mut self, id: usize, parent: usize) {
        if self.places[id].kind == DEVICE {
            let mut moved = self.places[id].clone();
            self.places[id].removed = true;
            moved.parent = Some(parent);
            self.places.push(moved);
        } else {
            self.places[id].parent = Some(parent);
        }
    }

    pub(super) fn workspace_xml(&self) -> String {
        let root = (0..self.places.len())
            .find(|id| self.places[*id].parent.is_none())
            .unwrap_or_default();
        format!(
            "<PHYSICALWORKSPACE>{}</PHYSICALWORKSPACE>",
            self.node_xml(root)
        )
    }

    fn node_xml(&self, id: usize) -> String {
        let place = &self.places[id];
        let children: String = self
            .children(id)
            .into_iter()
            .map(|child| self.node_xml(child))
            .collect();
        format!(
            "<NODE><X>{}</X><Y>{}</Y><TYPE>{}</TYPE><NAME translate=\"true\">{}</NAME><CHILDREN>{children}</CHILDREN><UUID_STR>{}</UUID_STR></NODE>",
            place.x,
            place.y,
            place.kind,
            quick_escape(&place.name),
            place.persistent
        )
    }

    pub(super) fn from_nodes(nodes: &[pktfile::PhysicalNode]) -> Self {
        let mut ordered: Vec<&pktfile::PhysicalNode> = nodes.iter().collect();
        ordered.sort_by_key(|node| node.depth);
        let mut places: Vec<Place> = Vec::new();
        for node in ordered {
            let parent = node
                .parent
                .as_ref()
                .and_then(|uuid| places.iter().position(|place| &place.persistent == uuid));
            #[allow(clippy::cast_possible_truncation)]
            places.push(Place {
                persistent: node.uuid.clone(),
                name: node.name.clone(),
                kind: i32::try_from(node.kind).unwrap_or_default(),
                x: node.x as i32,
                y: node.y as i32,
                parent,
                removed: false,
            });
        }
        Self {
            places,
            current: 0,
            cities_added: 0,
        }
    }

    /// Position used for radio range; the canvas treats local coordinates as global.
    pub(super) fn device_position(&self, name: &str) -> Option<(f64, f64)> {
        let id = self.device_place(name)?;
        Some((f64::from(self.places[id].x), f64::from(self.places[id].y)))
    }

    pub(super) fn parent_of_device(&self, name: &str) -> Option<String> {
        let id = self.device_place(name)?;
        self.places[id]
            .parent
            .map(|parent| self.places[parent].name.clone())
    }
}

pub(super) fn toolbar(state: &mut State, steps: &[Step]) -> Result<Value, Remote> {
    let [step] = steps else {
        return Err(Remote::unknown_method(TOOLBAR, ""));
    };
    no_args(step, TOOLBAR)?;
    let physical = &mut state.physical;
    match step.method.as_str() {
        "switchToTopView" => physical.current = 0,
        "switchToHomeRack" => physical.current = HOME_CLOSET,
        "addCity" if physical.current == 0 => {
            physical.cities_added += 1;
            let offset = 200 + 5 * physical.cities_added;
            physical.add("City", CITY, (offset, offset), 0);
        }
        "addCloset"
            if matches!(
                physical.places[physical.current].kind,
                UNIVERSE | CITY | BUILDING
            ) =>
        {
            let current = physical.current;
            physical.add("Wiring Closet", CLOSET, (50, 50), current);
        }
        "addCity" | "addCloset" => {}
        other => return Err(Remote::unknown_method(TOOLBAR, other)),
    }
    Ok(Value::Void)
}

pub(super) fn object(state: &mut State, id: usize, steps: &[Step]) -> Result<Value, Remote> {
    let [step, rest @ ..] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    let physical = &mut state.physical;
    match (step.method.as_str(), rest) {
        ("getChildAt", rest) => {
            let index = int_arg(step, CLASS)?;
            let child = usize::try_from(index)
                .ok()
                .and_then(|index| physical.children(id).get(index).copied())
                .ok_or_else(|| Remote::missing(CLASS))?;
            object(state, child, rest)
        }
        ("getChild", rest) => {
            let name = qstring_arg(step, CLASS)?;
            let child = physical
                .children(id)
                .into_iter()
                .find(|child| physical.places[*child].name == name)
                .ok_or_else(|| Remote::missing(CLASS))?;
            object(state, child, rest)
        }
        ("getParent", rest) => {
            no_args(step, CLASS)?;
            let parent = physical.places[id]
                .parent
                .ok_or_else(|| Remote::missing(CLASS))?;
            object(state, parent, rest)
        }
        ("getDevice", [name]) if physical.places[id].kind == DEVICE && name.method == "getName" => {
            let physical_name = physical.places[id].name.clone();
            state
                .devices
                .iter()
                .find(|device| device.physical_name == physical_name)
                .map(|device| Value::qstring(&device.name))
                .ok_or_else(|| Remote::missing("Device"))
        }
        (_, []) => attribute(physical, id, step),
        (other, _) => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn attribute(physical: &mut Physical, id: usize, step: &Step) -> Result<Value, Remote> {
    let place = &physical.places[id];
    match step.method.as_str() {
        "getName" => no_args(step, CLASS).map(|()| Value::qstring(&place.name)),
        "getType" => no_args(step, CLASS).map(|()| Value::Int(place.kind)),
        "getX" => no_args(step, CLASS).map(|()| Value::Int(place.x)),
        "getGlobalX" => no_args(step, CLASS).map(|()| Value::Double(f64::from(place.x))),
        "getGlobalY" => no_args(step, CLASS).map(|()| Value::Double(f64::from(place.y))),
        "getY" => no_args(step, CLASS).map(|()| Value::Int(place.y)),
        "getChildCount" => no_args(step, CLASS)
            .map(|()| Value::Int(i32::try_from(physical.children(id).len()).unwrap_or(i32::MAX))),
        "getObjectUuid" => no_args(step, CLASS).map(|()| Value::Uuid(format!("{{place-{id}}}"))),
        "getPathUuid" => no_args(step, CLASS).map(|()| Value::qstring(&place.persistent)),
        "moveTo" => {
            check_args(step, CLASS, &[TypeCode::Int, TypeCode::Int])?;
            let place = &mut physical.places[id];
            place.x = i32::try_from(step.args[0].as_i64().unwrap_or_default()).unwrap_or_default();
            place.y = i32::try_from(step.args[1].as_i64().unwrap_or_default()).unwrap_or_default();
            Ok(Value::Void)
        }
        "moveOutOfCurrentObject" => {
            no_args(step, CLASS)?;
            let Some(parent) = place.parent else {
                return Ok(Value::Bool(false));
            };
            let mut target = physical.places[parent].parent;
            if physical.places[parent].kind == RACK {
                target = target.and_then(|closet| physical.places[closet].parent);
            }
            let Some(target) = target else {
                return Ok(Value::Bool(false));
            };
            physical.relocate(id, target);
            Ok(Value::Bool(true))
        }
        "moveIntoObject" => {
            let name = qstring_arg(step, CLASS)?;
            let Some(parent) = place.parent else {
                return Ok(Value::Bool(false));
            };
            let Some(target) = physical
                .children(parent)
                .into_iter()
                .find(|sibling| *sibling != id && physical.places[*sibling].name == name)
            else {
                return Ok(Value::Bool(false));
            };
            let destination = if place.kind == DEVICE && physical.places[target].kind == CLOSET {
                physical.rack_in(target)
            } else {
                target
            };
            physical.relocate(id, destination);
            Ok(Value::Bool(true))
        }
        other => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn persistent_id(seed: usize) -> String {
    format!("{{00000000-0000-4000-8000-{seed:012}}}")
}

fn quick_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn by_uuid(state: &State, uuid: &str) -> Option<usize> {
    let id = uuid
        .strip_prefix("{place-")?
        .strip_suffix('}')?
        .parse::<usize>()
        .ok()?;
    state
        .physical
        .places
        .get(id)
        .filter(|place| !place.removed)
        .map(|_| id)
}
