# Coverage

Which parts of Packet Tracer pktctl reaches, and how each was validated.

Every remote method of the official IPC API (346 classes, 3111 methods,
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
| Canvas image and notes | `screenshot`, `add_note`, `list_notes`, `remove_note` | live |
| Any IPC method | `describe_ipc`, `call_ipc` | live |
| Physical workspace: tree, cities, closets, racks, moving devices and locations, view | `list_locations`, `add_location`, `move_to_location`, `show_workspace` | live |
| Physical workspace: renaming locations, creating buildings | none: not in the IPC API | planned through `.pkt` editing |
| Simulation mode, PDUs, event list | none yet | ipc |
| Device power | none yet | ipc |
| Wireless (SSID, security, association) | none yet | ipc |
| Server services (DHCP, DNS, HTTP, FTP, email, NTP, syslog, AAA) | none yet | ipc |
| Preferences and workspace options | none yet | ipc |
| Activity Wizard and assessment (`.pka`) | none yet | ipc |
| Multiuser, IoT, programming environment | none yet | ipc |
| IPC events (live notifications) | none yet | not reachable yet |
