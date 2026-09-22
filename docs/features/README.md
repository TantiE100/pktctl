# Features

Each MCP tool lives in its own folder under `crates/pktctl/src/features/`, with
its logic, its MCP adapter, its tests and a README.

| Feature | Tool | README |
|---|---|---|
| Catalog | `list_models` | [features/catalog](../../crates/pktctl/src/features/catalog/README.md) |
| Status | `status` | [features/status](../../crates/pktctl/src/features/status/README.md) |
| Devices | `list_devices`, `add_device`, `rename_device`, `move_device`, `remove_device` | [features/devices](../../crates/pktctl/src/features/devices/README.md) |
| CLI | `run_cli` | [features/cli](../../crates/pktctl/src/features/cli/README.md) |
| Host console | `run_host_command` | [features/host_console](../../crates/pktctl/src/features/host_console/README.md) |

Setup that is not a tool:

- [ExApp registration](exapp-registration.md): one-time step that lets pktctl
  authenticate with Packet Tracer.
