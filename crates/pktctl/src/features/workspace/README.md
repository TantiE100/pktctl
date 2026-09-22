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

Packet Tracer renders the logical workspace itself (`getWorkspaceImage`), so the
image matches what is on screen, labels included. The agent receives it as an
image it can look at; `save_to` also writes the PNG for lab reports.

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
