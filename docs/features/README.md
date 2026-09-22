# Features

Each MCP tool lives in its own folder under `crates/pktctl/src/features/`, with
its logic, its MCP adapter, its tests and a README.

| Feature | Tool | README |
|---|---|---|
| Catalog | `list_models` | [features/catalog](../../crates/pktctl/src/features/catalog/README.md) |
| Links | `list_ports`, `list_links`, `connect`, `disconnect` | [features/links](../../crates/pktctl/src/features/links/README.md) |
| Status | `status` | [features/status](../../crates/pktctl/src/features/status/README.md) |
| Devices | `list_devices`, `add_device`, `rename_device`, `move_device`, `remove_device` | [features/devices](../../crates/pktctl/src/features/devices/README.md) |
| CLI | `run_cli`, `configure_ios` | [features/cli](../../crates/pktctl/src/features/cli/README.md) |
| Hosts | `configure_host` | [features/hosts](../../crates/pktctl/src/features/hosts/README.md) |
| Host console | `run_host_command` | [features/host_console](../../crates/pktctl/src/features/host_console/README.md) |

Setup that is not a tool:

- [ExApp registration](exapp-registration.md): one-time step that lets pktctl
  authenticate with Packet Tracer.
