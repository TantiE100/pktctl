# physical

The physical workspace: Intercity, cities, buildings, wiring closets, racks and
where each device sits.

## Tools

| Tool | What it does |
|---|---|
| `list_locations` | Every location with its path, kind, position and the devices directly inside it. |
| `add_location` | Creates a city (in Intercity) or a wiring closet (in Intercity, a city or a building). |
| `move_to_location` | Moves a device or a whole location to another location, optionally to an `x`/`y` inside it. |
| `rename_location` | Renames any location, including `City#2`-style duplicates. |
| `add_building` | Creates a named building inside a city. |
| `show_workspace` | Switches the main window between the logical and the physical workspace. |

### Paths

Locations are addressed by their path from Intercity, with `/` between names:
`Home City/Corporate Office/Main Wiring Closet`. A leading `Intercity/` is
accepted, and Intercity itself is the empty path. When two siblings share a
name, `list_locations` suffixes the later ones: `City`, `City#2`.

```json
{ "device": "S1", "into": "Home City/Corporate Office/Main Wiring Closet" }
```

```json
{ "moved": "S1", "now_in": "Home City/Corporate Office/Main Wiring Closet/Rack" }
```

## What Packet Tracer allows, verified on 9.0.1

- **Creating**: the IPC API only offers the toolbar's *New City* and *New
  Closet* buttons. Both create in the current view, so the tool switches to the
  Intercity view, creates the location there and moves it into `inside`. There
  is no call to create buildings, racks or other containers.
- **Renaming and buildings are not in the IPC API.** New locations are named
  `City` and `Wiring Closet`, so creating two leaves duplicates, and Packet
  Tracer only moves things into the first of two same-named siblings. A move
  into `City#2` is refused with an explanation; rename it first.
- **Moving** is relative: `moveOutOfCurrentObject` goes one level up, or two
  when leaving a rack, and `moveIntoObject(name)` enters a sibling. The tool
  climbs to the common ancestor of the source and the destination, then enters
  each remaining level by name, and finally reads the tree back to confirm.
- **Racks**: a device moved into a wiring closet is mounted in its rack. If the
  closet has none, Packet Tracer creates one together with a new Power
  Distribution Device.
- **Device objects are recreated** on every move, with a new uuid, and take the
  device's name. The tool therefore always reaches a device through
  `network().getDevice(name).getPhysicalObject()` and locations by uuid,
  which stay stable.
- Physical objects keep the name the device had when it was created until the
  device moves, so `list_locations` reports devices by their current device
  name (`getDevice().getName()`).

## Editing the saved file

`rename_location` and `add_building` do what the IPC API cannot by editing the
network file with the [pktfile](../../../../pktfile/README.md) crate:

1. Save the network to its current file, or to a temporary file if it was
   never saved. The reply names the file.
2. Decode the `.pkt`, find the location by `UUID_STR` (the persistent id that
   `getPathUuid()` returns), and change only that node: its `NAME` text, or a
   new building `NODE` inside the city's `CHILDREN`. New buildings copy the
   defaults of the building in an empty Packet Tracer 9.0.1 network.
3. Encode the file and reopen it with `fileOpen`.
4. Remove the Power Distribution Devices that Packet Tracer adds on every
   open (one per rack), keeping every device that existed before.

The network therefore ends up saved. The file has to be readable by pktctl, so
these two tools need pktctl on the same computer as Packet Tracer.

## IPC calls

| Call | Use |
|---|---|
| `appWindow().getActiveWorkspace().getRootPhysicalObject()` | The Intercity node; the tree is read with `getChildCount`/`getChildAt`. |
| `PhysicalObject.getName/getType/getX/getY/getObjectUuid` | Node details; `getType` is the `PhysicalObjectType` enum. |
| `PhysicalObject.getPathUuid()` | The persistent id, equal to `UUID_STR` in the saved file. |
| `PhysicalObject.getDevice().getName()` | Device behind a device node. |
| `getObjectByUuid(uuid: string)` | Reaches a location again by uuid. |
| `appWindow().getPhysicalToolbar().switchToTopView/addCity/addCloset()` | Creating locations. |
| `PhysicalObject.moveOutOfCurrentObject()`, `moveIntoObject(name: QString)`, `moveTo(x: int, y: int)` | Moving. |
| `appWindow().getPLSwitch().showLogicalMode/showPhysicalMode()`, `appWindow().isPhysicalMode()` | The workspace shown. |
