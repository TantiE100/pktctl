# status

Tells the agent whether Packet Tracer is usable right now.

## Tool

`status`, no arguments, read-only.

```json
{ "connected": true, "pt_version": "9.0.1.0858", "devices": 11, "links": 9 }
```

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
