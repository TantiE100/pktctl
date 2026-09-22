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
| `status` | Is Packet Tracer reachable? Version, device and link counts, or what to fix. |
| `list_devices` | Every device with its name, model and kind. |
| `run_cli` | Runs one IOS command on a router or switch and returns the console output. |

## Quick start

1. Register pktctl as an ExApp in Packet Tracer once:
   [docs/features/exapp-registration.md](docs/features/exapp-registration.md).
2. Build: `make release`.
3. Add it to your MCP client:

```json
{
  "mcpServers": {
    "pktctl": {
      "command": "/path/to/pktctl/target/release/pktctl",
      "env": {
        "PKTCTL_APP_ID": "dev.pktctl",
        "PKTCTL_SECRET": "the KEY from your registration"
      }
    }
  }
}
```

## Documentation

Everything lives in [docs/](docs/README.md): architecture, features, the PTMP
wire reference and the development workflow.

## License

[MIT](LICENSE)
