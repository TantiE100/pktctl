# cli

Runs IOS commands on routers and switches, exactly as if typed in the device
console.

## Tool

`run_cli`

| Argument | Required | Meaning |
|---|---|---|
| `device` | yes | Device name as returned by `list_devices`. |
| `command` | yes | One IOS command. |
| `mode` | no | `user`, `enable` (default), `global` or `current`. |

```json
{ "device": "R1", "command": "show ip interface brief" }
```

```json
{ "status": "ok", "output": "Interface              IP-Address      OK? Method Status ..." }
```

`status` is IOS's verdict on the command: `ok`, `ambiguous`, `invalid`,
`incomplete` or `not_implemented`. A rejected command is still a successful
tool call; the agent reads `status` and `output` to correct itself.

Modes map to Packet Tracer's own: `global` enters configuration mode first,
`current` runs the command wherever the console already is, which is how to
continue inside `interface` or `router` sub-modes.

The tool is marked destructive because configuration commands change the
network.

## IPC calls

| Call | Reply |
|---|---|
| `network().getDevice(device: QString).enterCommand(command: string, mode: string)` | pair(int status, string output) |

Devices without an IOS console (PCs, servers) answer with an IPC error, which
reaches the agent as a tool error.
