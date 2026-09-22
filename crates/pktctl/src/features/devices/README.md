# devices

Lists every device in the open network.

## Tool

`list_devices`, no arguments, read-only.

```json
{
  "devices": [
    { "name": "R1", "model": "2911", "kind": "Router" },
    { "name": "SW1", "model": "2960-24TT", "kind": "CiscoDevice" },
    { "name": "PC1", "model": "PC-PT", "kind": "Pc" }
  ]
}
```

`kind` is Packet Tracer's own class name. Switches report `CiscoDevice` and
multilayer switches report `Router`, so use `model` when the distinction
matters.

## IPC calls

| Call | Reply |
|---|---|
| `network().getDeviceCount()` | int `n` |
| `network().getDeviceAt(i).getName()` for `i` in `0..n` | QString |
| `network().getDeviceAt(i).getModel()` | QString |
| `network().getDeviceAt(i).getClassName()` | QString |

All per-device calls are sent at once and matched by call id, so a large
network costs one round trip of latency, not `3n`.
