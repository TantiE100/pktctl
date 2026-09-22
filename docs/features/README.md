# Features

Each MCP tool lives in its own folder under `crates/pktctl/src/features/`, with
its logic, its MCP adapter, its tests and a README.

| Feature | Tool | README |
|---|---|---|
| Catalog | `list_models` | [features/catalog](../../crates/pktctl/src/features/catalog/README.md) |
| Links | `list_ports`, `list_links`, `connect`, `disconnect` | [features/links](../../crates/pktctl/src/features/links/README.md) |
| Modules | `list_slots`, `add_module`, `remove_module` | [features/modules](../../crates/pktctl/src/features/modules/README.md) |
| Workspace | `save_network`, `open_network`, `new_network`, `screenshot`, `add_note`, `list_notes`, `remove_note` | [features/workspace](../../crates/pktctl/src/features/workspace/README.md) |
| Status | `status` | [features/status](../../crates/pktctl/src/features/status/README.md) |
| Devices | `list_devices`, `add_device`, `rename_device`, `move_device`, `remove_device` | [features/devices](../../crates/pktctl/src/features/devices/README.md) |
| CLI | `run_cli`, `configure_ios` | [features/cli](../../crates/pktctl/src/features/cli/README.md) |
| Hosts | `configure_host`, `configure_host_ipv6`, `set_host_firewall` | [features/hosts](../../crates/pktctl/src/features/hosts/README.md) |
| Desktop apps | `browse_web`, `configure_email`, `send_email`, `receive_email`, `vpn_client`, `host_files` | [features/desktop](../../crates/pktctl/src/features/desktop/README.md) |
| Host console | `run_host_command` | [features/host_console](../../crates/pktctl/src/features/host_console/README.md) |
| Physical | `list_locations`, `add_location`, `rename_location`, `remove_location`, `arrange_devices`, `set_background`, `move_to_location`, `show_workspace` | [features/physical](../../crates/pktctl/src/features/physical/README.md) |
| Simulation | `simulation_mode`, `add_pdu`, `simulation_step`, `list_simulation_events` | [features/simulation](../../crates/pktctl/src/features/simulation/README.md) |
| Power | `set_power`, `fast_forward`, `power_cycle_all` | [features/power](../../crates/pktctl/src/features/power/README.md) |
| Wireless | `configure_access_point`, `connect_wireless`, `wireless_status` | [features/wireless](../../crates/pktctl/src/features/wireless/README.md) |
| Services | `list_server_services`, `set_server_service`, `configure_dhcp_server`, `configure_dns_server`, `set_web_page`, `add_server_user` | [features/services](../../crates/pktctl/src/features/services/README.md) |
| Preferences | `get_preferences`, `set_preferences` | [features/preferences](../../crates/pktctl/src/features/preferences/README.md) |
| Events | `watch_events` | [features/events](../../crates/pktctl/src/features/events/README.md) |
| Activity | `activity_status`, `activity_instructions`, `check_activity`, `reset_activity`, `unlock_activity`, `network_description` | [features/activity](../../crates/pktctl/src/features/activity/README.md) |
| IPC | `describe_ipc`, `call_ipc` | [features/ipc](../../crates/pktctl/src/features/ipc/README.md) |
| Setup | `setup_exapp` | [features/setup](../../crates/pktctl/src/features/setup/README.md) |

The whole registration story, including the manual path, is in
[ExApp registration](exapp-registration.md).
