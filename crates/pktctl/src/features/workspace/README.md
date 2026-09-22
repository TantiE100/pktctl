# workspace

Files, screenshots and canvas notes.

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `save_network` | optional absolute `path` (`.pkt`) | `{ path, bytes }` read back from disk |
| `open_network` | absolute `path`, optional `save_current_to` | `{ path, devices }` |
| `new_network` | optional `save_current_to` | `{ cleared: true }` |
| `screenshot` | optional absolute `save_to` (`.png`) | the PNG as MCP image content |
| `add_note` | `x`, `y`, `text` | the note with its id |
| `list_notes` | optional `include_port_labels` | notes with ids and positions |
| `remove_note` | `id` | `{ removed: id }` |

## No dialogs, ever

A modal dialog in Packet Tracer blocks it until someone clicks, which an agent
cannot do. These tools never trigger one:

- saving uses `fileSaveAsNoPrompt`, which never asks before overwriting;
- opening and starting a new network first call `fileNew(false)`, which
  discards the current network without the "save changes?" prompt. Pass
  `save_current_to` to keep the current work before it is discarded.

After saving, the file's existence and size are read back through Packet
Tracer's own file manager, so a missing folder or a write failure is reported
instead of assumed.

Packet Tracer 9.0.1 adds a "Power Distribution Device" to the physical
workspace every time a file is opened, so `devices` in the `open_network` reply
and `list_devices` include one more of them per open. pktctl reports them as
they are and never deletes them on its own; remove them with `remove_device`
if they get in the way.

## Screenshots

`screenshot` takes a `view`:

| View | How |
|---|---|
| `logical` (default) | Packet Tracer renders the logical workspace itself (`LogicalWorkspace.getWorkspaceImage("PNG")`). Works with the window hidden. |
| `physical` | Switches to the physical workspace at Intercity, captures Packet Tracer's window, switches back. |
| `physical_rack` | The same inside the main wiring closet, showing its rack. |
| `window` | Packet Tracer's window as it is, including any open dialog. |

The IPC API has no image of the physical workspace, so the last three capture
Packet Tracer's own window through the operating system with
[xcap](https://crates.io/crates/xcap) (macOS, Windows, Linux). Only that
window is captured, never the screen. Dialogs are separate windows, so
pktctl paints every Packet Tracer window that sits in front of the main one
onto it; a modal dialog, which stops Packet Tracer from answering IPC calls,
therefore shows up in a `window` capture even while every other tool times out. On macOS the app that runs pktctl needs
the Screen Recording permission (System Settings > Privacy & Security); the
error says so if it is missing. The capture waits 0.8 seconds after switching
views so Packet Tracer can redraw, and the view you had is restored afterwards.

## Notes and port labels

With "show port labels" on, Packet Tracer draws each cable end's port name
(`Gig0/0`, `Fa0/1`, `Se0/0/0`) as a canvas note. `list_notes` hides them by
matching the abbreviated names of linked ports; `include_port_labels: true`
lists everything. Positions come from `getCanvasItemRealX/Y`; the similarly
named `getCanvasItemX/Y` return offsets inside the item, not canvas
coordinates.

## IPC calls

| Purpose | Call |
|---|---|
| save | `appWindow().fileSaveAsNoPrompt(path: QString, async: bool)` |
| verify | `systemFileManager().fileExists(path: QString)`, `getFileSize(path: QString)` |
| current file | `appWindow().getActiveFile().getSavedFilename()` |
| new | `appWindow().fileNew(confirm: bool)` with `false` |
| open | `appWindow().fileOpen(path: QString)` returns a `FileOpenReturnValue` code, 0 on success |
| screenshot | `...getLogicalWorkspace().getWorkspaceImage(format: QString)` returns raw PNG bytes |
| notes | `...getLogicalWorkspace().getIncNoteZOrder()`, `addNote(x: int, y: int, layer: double, text: QString)`, `getCanvasNoteIds()`, `getCanvasNoteText(id: uuid)`, `getCanvasItemRealX/Y(id: uuid)`, `removeCanvasItem(id: uuid)` |

File operations need the `FILE` privilege, which pktctl's ExApp template
grants; see [ExApp registration](../../../../../docs/features/exapp-registration.md).
