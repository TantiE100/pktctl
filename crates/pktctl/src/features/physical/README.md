# physical

The physical workspace: Intercity, cities, buildings, wiring closets, racks and
where each device sits.

## Tools

| Tool | What it does |
|---|---|
| `list_locations` | Every location with its path, kind, position and the devices directly inside it. |
| `add_location` | Creates a city, a building, a wiring closet or furniture: `rack`, `table`, `shelf`, `cable_pegboard`, `container`. Cities and closets use Packet Tracer's buttons; the rest are written into the network file with the same fields its own buttons write. |
| `move_to_location` | Moves a device or a whole location to another location, optionally to an `x_percent`/`y_percent` inside it. |
| `rename_location` | Renames any location, including `City#2`-style duplicates. |
| `arrange_devices` | Lays devices out in rows inside a room, a building or a piece of furniture. |
| `set_background` | Papers a location, or the logical workspace, with one of Packet Tracer's backgrounds or an image of yours. |
| `remove_location` | Deletes a city, building, closet or rack with everything inside. Devices must be moved out first; the Power Distribution Devices Packet Tracer puts in racks are removed with it. |
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
- **Furniture takes devices only through the file.** Packet Tracer mounts
  anything dropped into a wiring closet in that closet's first rack, so its
  API cannot put a device on a table, a shelf or a second rack.
  `move_to_location` and `arrange_devices` write those moves into the network
  instead, keeping the three places Packet Tracer stores a device's physical
  path in step (`PHYSICAL`, `PARENT_PATH` and `CONTAINER_ID`); a file whose
  chains disagree is refused as *corrupted Physical Workspace data*.
- **Positions are percentages of the room.** Packet Tracer draws the contents
  of a container as a fraction of a fixed scene (3444 by 2157 units), not from
  metres, so `move_to_location` and `arrange_devices` take `x_percent` and
  `y_percent`: 50 and 50 is the middle. Measured on 9.0.1: a device written at
  20 percent lands at 20 percent of the room, and one written with raw
  coordinates ends up piled in the corner.
- **Colours**: Packet Tracer cannot paint a device's icon; `fillColor` only
  applies to IoT components. Rooms and the logical workspace take background
  images instead (`set_background`).
- **Racks and tables**: a device moved into a wiring closet lands where
  Packet Tracer puts it: in the rack of the default closets, which gains a new
  Power Distribution Device when it has none, or on the table of a closet made
  with `add_location` (verified on 9.0.1). `now_in` names the exact place.
- **Device objects are recreated** on every move, with a new uuid, and take the
  device's name. The tool therefore always reaches a device through
  `network().getDevice(name).getPhysicalObject()` and locations by uuid,
  which stay stable.
- Physical objects keep the name the device had when it was created until the
  device moves, so `list_locations` reports devices by their current device
  name (`getDevice().getName()`).

## Editing the network file

`rename_location`, `add_building` and `remove_location` do what the IPC API cannot by editing the
network with the [pktfile](../../../../pktfile/README.md) crate:

1. Take the open network as `.pkt` bytes with `AppWindow.fileSaveToBytes`.
   Nothing is written to disk and the open file is not saved.
2. Decode them, find the location by `UUID_STR` (the persistent id that
   `getPathUuid()` returns), and change only that node: its `NAME` text, or a
   new building `NODE` inside the city's `CHILDREN`. New buildings copy the
   defaults of the building in an empty Packet Tracer 9.0.1 network.
3. Write the result to a new temporary file and open it with `fileOpen`.
4. Remove the Power Distribution Devices that Packet Tracer adds on every
   open (one per rack), keeping every device that existed before.
5. Fast forward time. Opening a file boots the network again: switch ports
   restart spanning tree and hosts ask DHCP again, and a PC can come back as
   0.0.0.0 (measured on 9.0.1). `fastForwardTime` settles both at once.

**Your own file is never written.** Packet Tracer ends up with the temporary
copy open, and the reply's `file` names it; there is no IPC call to point the
open network back at your file, so keep the change with `save_network` and
the path you want. The temporary file must be on the computer that runs
Packet Tracer, so these tools need pktctl there too.

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
