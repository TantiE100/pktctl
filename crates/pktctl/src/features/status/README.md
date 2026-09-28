# status

Tells the agent whether Packet Tracer is usable right now.

## Tool

`status`, no arguments, read-only.

```json
{ "connected": true, "addr": "127.0.0.1:39000", "pt_version": "9.0.1.0858", "devices": 11, "links": 9 }
```

`addr` is where Packet Tracer answered. Without `PKTCTL_ADDR`, pktctl looks on
ports 39000 to 39009, starting with the port that worked last, and only accepts
a port where the PTMP handshake with its credentials completes. When nothing
answers, `problem` names the range searched; when something answered but
refused, it reports that instead (a rejected app id before a busy Packet Tracer
before a closed port).

```json
{ "connected": false, "problem": "Packet Tracer is not reachable (...); open Packet Tracer and retry" }
```

The tool never fails: an unreachable or unregistered Packet Tracer is a normal
answer, reported in `problem`, so the agent can guide the user.

## IPC calls

Issued concurrently:

| Call | Reply |
|---|---|
| negotiated version (from the handshake) | `:PTVER` suffix |
| `network().getDeviceCount()` | int |
| `network().getLinkCount()` | int |
