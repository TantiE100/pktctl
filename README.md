# pktctl

MCP server for Cisco Packet Tracer. It talks to a running Packet Tracer through
PTMP, the native IPC protocol Packet Tracer ships for external applications, so
there is no extension window to keep open and no polling bridge in between.

- One static binary, no runtime to install.
- Calls round-trip in well under a millisecond and can be pipelined.
- Errors come back typed (`Device: IPC Cache entry`), never as a frozen modal.

## Tools

| Tool | What it does |
|---|---|
| `setup_exapp` | Creates the one-time Packet Tracer registration file for pktctl. |
| `status` | Is Packet Tracer reachable? Version, device and link counts, or what to fix. |
| `list_devices` | Every device with its name, model, kind and position. |
| `add_device`, `rename_device`, `move_device`, `remove_device` | Build and reshape the topology. |
| `list_ports`, `list_links`, `connect`, `disconnect` | Inspect ports and cable devices together, with CCNA cable selection. |
| `list_models` | Device and module models available in this Packet Tracer. |
| `run_cli` | Types one IOS command at a router or switch console and returns its complete output, `ping` and `traceroute` included. |
| `configure_ios` | Applies a block of IOS configuration, stops at the first rejected command, optionally saves. |
| `list_slots`, `add_module`, `remove_module` | Inspect slots and install cards such as HWIC-2T, with the power cycle handled. |
| `configure_host` | Static IP, mask, gateway and DNS, or DHCP, on PCs and servers, with CCNA sanity checks. |
| `save_network`, `open_network`, `new_network` | Files, with no dialog that could freeze Packet Tracer. |
| `screenshot`, `add_note`, `list_notes`, `remove_note` | See the canvas and annotate it. |
| `run_host_command` | Runs a Command Prompt command (ping, ipconfig, tracert) on a PC or server. |
| `list_locations`, `add_location`, `add_building`, `rename_location`, `move_to_location`, `show_workspace` | Physical workspace: cities, buildings, closets, racks and where each device sits. |
| `simulation_mode`, `add_pdu`, `simulation_step`, `list_simulation_events` | Simulation mode with the per-hop event list and Packet Tracer's own explanations. |
| `set_power`, `fast_forward`, `power_cycle_all` | Power and Realtime time controls; `fast_forward` makes STP, DHCP and routing converge at once. |
| `configure_access_point`, `connect_wireless`, `wireless_status` | Wi-Fi: SSID and WPA2/WPA/WEP on access points, clients that really associate. |
| `describe_ipc`, `call_ipc` | The whole Packet Tracer IPC API (346 classes, 3111 methods), searchable and callable with exact types. |

Dedicated tools cover everyday work; `call_ipc` reaches everything else.
[docs/coverage.md](docs/coverage.md) tracks which Packet Tracer areas have a
dedicated tool and how each one was validated.

## Quick start

1. Build: `make release`.
2. Pick an app id and a random secret (`openssl rand -hex 24`) and add pktctl to
   your MCP client:

```json
{
  "mcpServers": {
    "pktctl": {
      "command": "/path/to/pktctl/target/release/pktctl",
      "env": {
        "PKTCTL_APP_ID": "dev.pktctl",
        "PKTCTL_SECRET": "your random secret"
      }
    }
  }
}
```

3. Ask the agent to run `setup_exapp`, then register the `.pta` it creates in
   Packet Tracer once (**Extensions → IPC → Configure Apps → Add**). Details in
   [docs/features/exapp-registration.md](docs/features/exapp-registration.md).

## Documentation

Everything lives in [docs/](docs/README.md): architecture, features, the PTMP
wire reference and the development workflow.

## License

[MIT](LICENSE)
