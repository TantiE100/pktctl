# Changelog

Notable changes to pktctl, `pktfile` and `ptmp`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html). The three crates
share one version.

## Unreleased

### Changed

- The IPC index generator is a Rust crate, `tools/ipc-index`, and reads the
  class files out of the framework jar itself. It needs no JDK and no `javap`,
  and `make check` now compiles, lints and tests it with everything else.

### Fixed

- A method taking a generic argument was recorded as taking two: the old
  generator split the printed Java types on every comma, so
  `OSPFAreaNetwork.setIpAndMask(Pair<IPAddress,IPAddress>)` came out with two
  parameters and a parameter named `Pair<IPAddress`.

## 0.1.0 - 2026-09-22

First working version: 72 tools that cover Packet Tracer's logical and physical
workspaces, validated against Packet Tracer 9.0.1 on macOS.

### Added

- `ptmp`: async client for the Packet Tracer Messaging Protocol. Handshake and
  HMAC authentication, framing, every wire type including value objects and
  their data layouts, event subscription, and `ptmp::fake::FakePt` for tests.
- `pktfile`: `.pkt` codec (byte scrambling, Twofish-EAX, masking, Qt zlib) and
  byte-range edits of the physical workspace, for the renames, buildings and
  furniture the IPC API cannot do.
- `pktctl`: the MCP server.
  - Topology: `add_device`, `rename_device`, `move_device`, `remove_device`,
    `list_devices`, `list_models`, `list_ports`, `list_links`, `connect`,
    `disconnect`, with CCNA cable selection.
  - Consoles: `run_cli` with pending questions such as `[confirm]` and
    `Password:`, `configure_ios`, `run_host_command`.
  - Hosts: `configure_host`, `configure_host_ipv6`, `set_host_firewall`, and
    the Desktop apps through `browse_web`, `configure_email`, `send_email`,
    `receive_email`, `vpn_client`, `host_files`.
  - Servers: `list_server_services`, `set_server_service`,
    `configure_dhcp_server`, `configure_dns_server`, `set_web_page`,
    `add_server_user`.
  - Wireless: `configure_access_point`, `connect_wireless`, `wireless_status`.
  - Physical workspace: `list_locations`, `add_location`, `rename_location`,
    `remove_location`, `move_to_location`, `arrange_devices`, `set_background`,
    `show_workspace`, with placement in percentages of the room.
  - Canvas: `screenshot`, `add_note`, `list_notes`, `remove_note`, `draw`,
    `list_drawings`, `remove_drawing`.
  - Simulation and power: `simulation_mode`, `add_pdu`, `simulation_step`,
    `list_simulation_events`, `set_power`, `fast_forward`, `power_cycle_all`.
  - Files and activities: `save_network`, `open_network`, `new_network`,
    `activity_status`, `activity_instructions`, `check_activity`,
    `reset_activity`, `unlock_activity`, `network_description`.
  - Escape hatches: `describe_ipc` and `call_ipc` over the whole IPC API
    (346 classes, 3111 methods), `watch_events`, `setup_exapp`, `status`,
    `get_preferences`, `set_preferences`.

### Notes

- No Packet Tracer code, file or documentation is redistributed. The IPC index
  carries signatures only; `cargo run -p ipc-index -- ... --with-summaries`
  rebuilds a local one with Cisco's Javadoc prose for your own use.
- Windows and Linux are untested: every live check so far ran on macOS.
