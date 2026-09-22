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
| Canvas image and notes | `screenshot`, `add_note`, `list_notes`, `remove_note` | live |
| Any IPC method | `describe_ipc`, `call_ipc` | live |
| Physical workspace: tree, cities, closets, racks, moving devices and locations, view | `list_locations`, `add_location`, `move_to_location`, `show_workspace` | live |
| Physical workspace: renaming locations, creating buildings (not in the IPC API) | `rename_location`, `add_building`, through `.pkt` editing | live |
| Simulation mode, simple PDUs, stepping, event list with decisions | `simulation_mode`, `add_pdu`, `simulation_step`, `list_simulation_events` | live |
| Complex PDUs, scenarios, event-list GUI filters, play speed | none yet | ipc |
| Device power, fast forward, power cycling | `set_power`, `fast_forward`, `power_cycle_all` | live |
| Wireless: access point security, client association, status | `configure_access_point`, `connect_wireless`, `wireless_status` | live |
| Wireless: channels, radio bands, MAC filtering, WLC, cellular | none yet | ipc |
| Server services: DHCP, DNS, HTTP/HTTPS, FTP, email, NTP, Syslog, TFTP | `list_server_services`, `set_server_service`, `configure_dhcp_server`, `configure_dns_server`, `set_web_page`, `add_server_user` | live |
| Server services: AAA (RADIUS/TACACS+), IoT server, NTP authentication, Syslog entries | none yet | ipc |
| Preferences | `get_preferences`, `set_preferences` | live |
| Background images, recent files, custom hide options, buffer-full action | none yet | ipc |
| Activity Wizard and assessment (`.pka`) | none yet | ipc |
| Multiuser, IoT, programming environment | none yet | ipc |
| IPC events: 73 classes, 202 events | `watch_events` | live |
