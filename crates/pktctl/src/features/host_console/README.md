# host_console

Runs commands in the Command Prompt of end devices (PCs, laptops, servers):
`ping`, `ipconfig`, `tracert`, `nslookup`, `arp -a`.

## Tool

`run_host_command`

| Argument | Required | Meaning |
|---|---|---|
| `device` | yes | Host name as returned by `list_devices`. |
| `command` | yes | Command Prompt command. |
| `timeout_secs` | no | Seconds to wait for the command to end. Default 30, maximum 300. |

```json
{ "device": "PC1", "command": "ping -n 2 192.168.9.11" }
```

```json
{
  "finished": true,
  "status": "ok",
  "output": "ping -n 2 192.168.9.11\n\nPinging 192.168.9.11 with 32 bytes of data:\n\nReply from 192.168.9.11: bytes=32 time<1ms TTL=128\n..."
}
```

`finished: false` means the timeout elapsed first; `output` then holds what the
command printed so far. Routers and switches have no Command Prompt; the error
points to `run_cli`.

## How it works

The Command Prompt does not return its output from the call that types the
command. Output arrives as events, so the tool:

1. Resolves the terminal: `network().getDevice(device).getCommandPrompt().getObjectUuid()`.
2. Subscribes to `TerminalLine` events `outputWritten` and `commandEnded` for
   that uuid. The event receiver is created before the subscription is sent,
   so no output can be missed.
3. Types the command: `...getCommandPrompt().enterCommand(command: string)`.
4. Appends every `outputWritten` text until `commandEnded`, whose second
   argument is the command status.
5. Unsubscribes (same subscription with `false`).

Events for other terminals are ignored, so two agents can use different PCs at
the same time.
