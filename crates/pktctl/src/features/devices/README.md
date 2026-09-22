# devices

Everything about devices as a whole: listing, creating, renaming, moving and
deleting them.

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `list_devices` | none | every device |
| `add_device` | `model`, optional `name`, optional `x` and `y` | the new device |
| `rename_device` | `name`, `new_name` | the renamed device |
| `move_device` | `name`, `x`, `y` | the moved device |
| `remove_device` | `name` | `{ "removed": name }` |

A device is reported as Packet Tracer shows it after the change, read back
with fresh calls:

```json
{ "name": "R1", "model": "2911", "kind": "router", "x": 250.0, "y": 80.0 }
```

- `kind` is the device type from Packet Tracer's catalog (`router`, `switch`,
  `multi_layer_switch`, `pc`, `server`, ...), the same names `list_models` uses.
- `x` and `y` are the **center** of the device icon on the logical canvas, for
  both input and output. Packet Tracer may round a new device's center by one
  pixel.
- Packet Tracer counts infrastructure objects as devices too; a network with a
  rack always contains a `Power Distribution Device`.

## Behaviour

- `add_device` resolves `model` through the catalog (exact, then ignoring case,
  spaces and dashes, so `isr 4331` finds `ISR4331`) and suggests close matches
  otherwise. Without `x`/`y` it uses the next slot of an 8-column grid.
- Routers and switches skip their boot sequence, so `run_cli` works right away,
  and their console is taken past the "initial configuration dialog" to the
  user prompt (`Router>`, `Switch>`), so opening the CLI tab in Packet Tracer
  shows a usable console.
- Names are unique: `add_device` and `rename_device` refuse a name that is
  already taken. Packet Tracer itself would accept the duplicate and leave one
  device unreachable by name.
- Operations on a missing device fail with ``device `X` not found``.

## IPC calls

| Purpose | Call |
|---|---|
| count | `network().getDeviceCount()` |
| read | `network().getDevice(name: QString)` or `getDeviceAt(i: int)`, then `getName()`, `getModel()`, `getType()`, `getCenterXCoordinate()`, `getCenterYCoordinate()` |
| create | `appWindow().getActiveWorkspace().getLogicalWorkspace().addDevice(type: int, model: string, x: double, y: double)` returns the generated name, or empty on failure |
| skip boot | `network().getDevice(name).skipBoot()` (IOS devices only) |
| console | `network().getDevice(name).getCommandLine().getPrompt()`, then `enterCommand("no")` on the initial dialog and `enterCommand("")` for the first Return |
| rename | `network().getDevice(name).setName(new: QString)` |
| move | `network().getDevice(name).moveToLocationCentered(x: int, y: int)` returns bool |
| delete | `...getLogicalWorkspace().removeDevice(name: QString)` returns bool |

`addDevice` places the device by its center while `getXCoordinate` reports the
top-left corner, which is why pktctl reads and moves by center only.

Per-device reads are pipelined, so listing a large network costs one round trip
of latency.
