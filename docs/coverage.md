# Coverage

Which parts of Packet Tracer pktctl reaches, and how each was validated.

Value objects (ACL statements, ARP tables, flowchart nodes and 356 more
classes) come back with their field names. Every remote method of the official IPC API (346 classes, 3111 methods,
133 enums in Packet Tracer 9.0.1) is callable through `call_ipc`, with argument
types checked against the framework before anything is sent. The table below
tracks which areas also have **dedicated tools**: friendlier arguments, CCNA
rules, multi-step workflows and README documentation.

Validation levels:

- **live**: exercised against a running Packet Tracer 9.0.1, in
  `crates/pktctl/tests/live.rs` or a recorded manual session.
- **canvas**: covered by unit and MCP end-to-end tests on the in-memory canvas.
- **ipc**: reachable through `call_ipc` only.

| Area | Dedicated tools | Validation |
|---|---|---|
| Connection and registration | `status`, `setup_exapp` | live |
| Hardware catalog | `list_models` | live |
| Logical topology: devices | `list_devices`, `add_device`, `rename_device`, `move_device`, `remove_device` | live |
| Cabling | `list_ports`, `list_links`, `connect`, `disconnect` | live |
| Modules and slots | `list_slots`, `add_module`, `remove_module` | live |
| IOS console and configuration | `run_cli`, `configure_ios` | live |
| End-device addressing | `configure_host` | live |
| End-device Command Prompt | `run_host_command` | live |
| Files | `save_network`, `open_network`, `new_network` | live |
| Canvas image and notes; physical workspace and window captures | `screenshot`, `add_note`, `list_notes`, `remove_note` | live |
| Any IPC method | `describe_ipc`, `call_ipc` | live |
| Physical workspace: tree, cities, closets, racks, moving devices and locations, view | `list_locations`, `add_location`, `move_to_location`, `show_workspace` | live |
| Physical workspace: renaming locations, creating buildings, deleting locations (not in the IPC API) | `rename_location`, `add_building`, `remove_location`, through `.pkt` editing | live |
| Simulation mode, simple PDUs, stepping, event list with decisions | `simulation_mode`, `add_pdu`, `simulation_step`, `list_simulation_events` | live |
| Complex PDUs, scenarios, event-list GUI filters, play speed | `call_ipc` | ipc |
| Device power, fast forward, power cycling | `set_power`, `fast_forward`, `power_cycle_all` | live |
| Wireless: access point security, client association with range diagnosis, status | `configure_access_point`, `connect_wireless`, `wireless_status` | live |
| Wireless: channels, radio bands, MAC filtering, WLC, cellular | `call_ipc` | ipc |
| Server services: DHCP, DNS, HTTP/HTTPS, FTP, email, NTP, Syslog, TFTP | `list_server_services`, `set_server_service`, `configure_dhcp_server`, `configure_dns_server`, `set_web_page`, `add_server_user` | live |
| Server services: AAA (RADIUS/TACACS+), IoT server, NTP authentication, Syslog entries | `call_ipc` | ipc |
| Preferences | `get_preferences`, `set_preferences` | live |
| Background images, recent files, custom hide options, buffer-full action | `call_ipc` | ipc |
| Activities (`.pka`): status, instructions, connectivity checks, reset, password unlock; file description | `activity_status`, `activity_instructions`, `check_activity`, `reset_activity`, `unlock_activity`, `network_description` | live |
| Activity authoring: wizard, answer network, variables, scripts, timers, passwords | `call_ipc` | ipc |
| Multiuser, IoT, programming environment | `call_ipc` | ipc |
| IPC events: 73 classes, 202 events | `watch_events` | live |

## Limits of Packet Tracer's IPC API

What the API does not offer, and how pktctl handles it:

| Need | In the IPC API? | pktctl |
|---|---|---|
| Rename a physical location, create a building, delete a location | No | `rename_location`, `add_building`, `remove_location` take the network as bytes (`fileSaveToBytes`), edit them and open the result as a temporary copy; your file is never written. |
| Connect a wireless client to a chosen network | `setCurrentProfile` exists but fails in 9.0.1, and clients only associate when their radio starts | `connect_wireless` does the same with the client's current profile. |
| Point the open network back at your own file after such an edit | No call sets the open file's name | The reply names the temporary copy; `save_network` with your path keeps the change. |
| Register pktctl as an external application | No | `setup_exapp` builds the file; adding it is one click in Packet Tracer. |
| Image of the physical workspace | No, only `LogicalWorkspace.getWorkspaceImage` | `screenshot` with `view: physical` or `physical_rack` switches views and captures Packet Tracer's own window through the operating system. |
| Headless Packet Tracer on macOS | No: the Cocoa platform plugin is required | Packet Tracer must be running with a window. |

Everything else Packet Tracer exposes over IPC is callable through
`call_ipc`, and every event through `watch_events`.
