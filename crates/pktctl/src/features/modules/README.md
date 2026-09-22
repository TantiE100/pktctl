# modules

Expansion modules: serial cards (HWIC-2T, NIM-2T), switch cards, SFPs.

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `list_slots` | `device` | removable slots, what each accepts and holds, and the supported module models |
| `add_module` | `device`, `slot`, `module` | the change, including the ports the module added |
| `remove_module` | `device`, `slot` | the change, including the ports and links that disappeared |

```json
{ "device": "R1", "slots": [ { "path": "0/0", "accepts": "interface_card" }, { "path": "0/1", "accepts": "interface_card", "installed": "HWIC-2T" } ],
  "supported_modules": ["GLC-LH-SMD", "HWIC-1GE-SFP", "HWIC-2T", "HWIC-4ESW", "HWIC-8A", "WIC-Cover"] }
```

```json
{ "device": "R1", "slot": "0/0", "module": "HWIC-2T", "ports_added": ["Serial0/0/0", "Serial0/0/1"], "ports_removed": [] }
```

Slot paths are what Packet Tracer uses: `0/0` to `0/3` for HWIC slots on ISR G2
routers (a 1941 has `0/0` and `0/1`), `0/1` and `0/2` for NIM slots on ISR 4000
routers, `0/0/0` and `0/0/1` for their built-in SFP cages. `list_slots` shows the
real ones for any device, so nothing has to be guessed.

## Behaviour

- Everything is validated before the device is touched: the module must be in
  the device's supported list, the slot must exist and be empty, and the slot
  must accept that kind of module (the error lists the free slots that fit).
- Packet Tracer refuses module changes on a powered device, just like real
  hardware. The tool powers the device off, changes the module, powers it back
  on, skips the boot sequence and brings the console to the user prompt.
- The power cycle discards unsaved configuration. Save first with
  `configure_ios` (`save: true`) if the running configuration matters.
- Removing a module that has cables attached removes those links too;
  `cut_links` lists them.

## IPC calls

| Purpose | Call |
|---|---|
| slots | `network().getDevice(d).getRootModule()` then, recursively, `getSlotCount()`, `getSlotTypeAt(i: int)`, `getModuleAt(i: int).getDescriptor().getModel()` (an empty slot answers `IPC Cache entry`) |
| supported | `network().getDevice(d).getSupportedModule()` returns strings `MODEL:image-path description` |
| power | `network().getDevice(d).setPower(on: bool)`, then `skipBoot()` |
| install | `network().getDevice(d).addModule(slot: string, type: int, model: string)` returns bool |
| remove | `network().getDevice(d).removeModule(slot: string)` returns bool |

Slot types and module kinds are `ModuleType` values (`interface_card` 2,
`network_module` 1, `sfp_module` 30). Built-in boards are
`non_removable_module` and are walked through but not listed.
