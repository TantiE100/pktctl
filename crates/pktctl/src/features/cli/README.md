# cli

IOS on routers and switches: single commands and whole configuration blocks.

## Tools

### `run_cli`

| Argument | Required | Meaning |
|---|---|---|
| `device` | yes | Device name as returned by `list_devices`. |
| `command` | yes | One IOS command. |
| `mode` | no | `user`, `enable` (default), `global` or `current`. |
| `timeout_secs` | no | Seconds to wait for the command to end. Default 30, maximum 300. |
| `password` | no | Console line password, when the device asks `Password:` on the console. |
| `enable_password` | no | Privileged mode password (`enable secret`). |

```json
{ "device": "R1", "command": "ping 192.168.10.11" }
```

```json
{
  "finished": true,
  "status": "ok",
  "output": "Type escape sequence to abort.\nSending 5, 100-byte ICMP Echos to 192.168.10.11, timeout is 2 seconds:\n!!!!!\nSuccess rate is 100 percent (5/5), round-trip min/avg/max = 0/0/0 ms\n"
}
```

`status` is IOS's verdict on the command: `ok`, `ambiguous`, `invalid`,
`incomplete` or `not_implemented`. A rejected command is still a successful
tool call; the agent reads `status` to correct itself. `finished: false` means
the command was still running when `timeout_secs` elapsed; the tool stopped it
with Ctrl+Shift+6, the IOS escape sequence, so the console is free again.

The command is typed at the device's real console, the one in the CLI tab, so
commands that print over time (`ping`, `traceroute`) come back complete, long
output such as `show running-config` comes back whole without `--More--`
pages, and the user sees everything the agent typed. Before typing, the tool
moves the console to `mode`: `enable`, `disable`, `end` and
`configure terminal` as needed. If `enable` asks for a password the tool stops
and says so rather than guessing one.

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
- **Locked consoles.** A device with `line console 0` + `password`, or with
  `enable secret`, asks before letting anything through. Give `password` and
  `enable_password` and the tool types them where IOS asks; without them the
  error names the field to fill in and leaves the console back at its prompt.
  `configure_ios` does not need them: Packet Tracer's `enterCommand` applies
  configuration without going through the console login (verified on 9.0.1).
- **Questions stay open.** When the output ends in a question (`[confirm]`,
  `[yes/no]:`, `Destination filename [startup-config]?`, `Password:`), the
  reply has `finished: false` and `question`, and nothing is interrupted.
  Answer with another `run_cli` in mode `current`: `yes`, a file name, or an
  empty command for Enter. Until then other modes are refused, because typing
  `end` would answer the question.
- **Reloads.** `reload` asks `[confirm]`; answering boots the device again,
  and the next `run_cli` skips the rest of the boot (`isBooting`,
  `skipBoot`) and presses Enter at *Press RETURN to get started*.

## IPC calls

`run_cli` drives the console line, `network().getDevice(device: QString).getCommandLine()`,
through the shared [terminal](../terminal.rs) helper, which also serves
`run_host_command`:

| Call | Reply |
|---|---|
| `...getCommandLine().getMode()` | string: `user`, `enable`, `global`, `intG`, ... |
| `...getCommandLine().getObjectUuid()` | uuid used to subscribe to its events |
| `...getCommandLine().enterCommand(command: string)` | void; output arrives as `TerminalLine` events |

The console echoes every typed character as `outputWritten`; the echo of the
command is removed from `output`. `commandEnded` carries the status. The prompt
printed after it is left out.

`configure_ios` uses a different call:

| Call | Reply |
|---|---|
| `network().getDevice(device: QString).enterCommand(command: string, mode: string)` | pair(int status, string output) |

`mode` is `user`, `enable`, `global`, or empty for the current mode. This call
answers synchronously with each command's status, which is what a
configuration block needs. It does not wait for output that arrives later, so
`ping` sent this way returns an empty line; that is why `run_cli` uses the
console instead.
