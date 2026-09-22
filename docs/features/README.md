# Features

Each MCP tool lives in its own folder under `crates/pktctl/src/features/`, with
its logic, its MCP adapter, its tests and a README.

| Feature | Tool | README |
|---|---|---|
| Status | `status` | [features/status](../../crates/pktctl/src/features/status/README.md) |
| Devices | `list_devices` | [features/devices](../../crates/pktctl/src/features/devices/README.md) |
| CLI | `run_cli` | [features/cli](../../crates/pktctl/src/features/cli/README.md) |

Setup that is not a tool:

- [ExApp registration](exapp-registration.md): one-time step that lets pktctl
  authenticate with Packet Tracer.
