# cli

IOS on routers and switches: single commands and whole configuration blocks.

## Tools

### `run_cli`

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
tool call; the agent reads `status` to correct itself. Long output such as
`show running-config` comes back whole, without `--More--` pages.

### `configure_ios`

| Argument | Required | Meaning |
|---|---|---|
| `device` | yes | Router or switch name. |
| `commands` | yes | Configuration commands in order. |
| `save` | no | Run `write memory` when every command was accepted. |

```json
{
  "device": "R1",
  "commands": ["interface GigabitEthernet0/0", "ip address 192.168.10.1 255.255.255.0", "no shutdown"],
  "save": true
}
```

```json
{
  "device": "R1", "completed": true, "applied": 3, "total": 3, "saved": true,
  "results": [
    { "command": "interface GigabitEthernet0/0", "status": "ok" },
    { "command": "ip address 192.168.10.1 255.255.255.0", "status": "ok" },
    { "command": "no shutdown", "status": "ok" }
  ]
}
```

- The first command runs in `global` mode, which enters configuration mode;
  the rest run in the current mode, so `interface`, `router ospf` or `line`
  sub-modes behave exactly as typed at the console.
- A leading `enable` / `configure terminal` (any usual abbreviation) and a
  trailing `end` are dropped, so pasted configuration works as is.
- The block stops at the first command IOS rejects. `results` shows every
  command that ran, the last one with its failing status; `completed` is
  `false` and nothing is saved.
- `end` is always sent afterwards, so the device is never left in
  configuration mode.

## Things worth knowing

- `hostname` changes the IOS prompt, not the device's name on the canvas; use
  `rename_device` for that.
- A switch port that just came up runs spanning tree (listening, learning)
  before it forwards, so the first pings through a new link can time out for
  up to about 30 seconds.
- PCs and servers have no IOS console; the error points to `run_host_command`.

## IPC calls

| Call | Reply |
|---|---|
| `network().getDevice(device: QString).enterCommand(command: string, mode: string)` | pair(int status, string output) |

`mode` is `user`, `enable`, `global`, or empty for the current mode. This call
bypasses the console line, so it works even while the console is still showing
the initial configuration dialog.
