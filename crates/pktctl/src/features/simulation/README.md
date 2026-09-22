# simulation

Simulation mode: send PDUs, step through time, and read what happened at every
hop with Packet Tracer's own explanations.

## Tools

| Tool | What it does |
|---|---|
| `simulation_mode` | `{ "on": true }` for Simulation, `false` for Realtime. Returns the mode, the simulation clock and the number of recorded events. |
| `add_pdu` | The *Add Simple PDU* button: an ICMP echo from `source` to `destination`. Sent at once in Realtime; queued in Simulation. |
| `simulation_step` | `forward` (*Capture/Forward*), `back`, or `reset` the event list; `times` repeats forward/back up to 200. |
| `list_simulation_events` | The event list, filtered by `protocols` and `device`, newest last, optionally with each step's decisions. |

```json
{ "protocols": ["ICMP"], "device": "PC1", "include_decisions": true, "limit": 5 }
```

```json
{
  "events": [
    {
      "index": 19, "time": 5, "device": "PC2", "from": "SW", "protocol": "ICMP",
      "status": [],
      "decisions": [
        "FastEthernet0 receives the frame.",
        "The packet is an ICMP packet. The ICMP process processes it.",
        "The ICMP process replies to the Echo Request by setting ICMP type to Echo Reply."
      ]
    }
  ],
  "matching": 6,
  "total": 22
}
```

- `protocol` is the `TrafficType` enum without its prefix: `ICMP`, `ARP`,
  `STP`, `DHCP`, `OSPF`, `RIP_V2`, ... The filter is applied by pktctl, so the
  event-list filters in Packet Tracer's window are left untouched.
- `status` lists what Packet Tracer flags for the event: `accepted`,
  `dropped`, `buffered`, `in_transit`, `collided`.
- `decisions` is the text of the PDU Details window, layer by layer.
- Switch ports that just came up run spanning tree first; send the PDU once the
  link is forwarding (a Realtime ping that gets replies is a good check).
- `add_pdu` errors come from Packet Tracer's `ADD_PDU_ERROR`: missing source
  or destination device, or a device without an IP address.

## IPC calls

| Call | Use |
|---|---|
| `simulation().setSimulationMode(bool)`, `isSimulationMode()`, `getCurrentSimTime()` | Mode and clock. |
| `simulation().forward()`, `backward()`, `resetSimulation()` | Stepping. |
| `simulation().getFrameInstanceCount()`, `getFrameInstanceAt(int)` | The event list. |
| `FrameInstance.getTime/getDevice/getPreviousDevice/getUserTrafficType/getSourceString/getDestinationString/isFrame*` | One event. |
| `FrameInstance.getFlowChartNodeCount()`, `getDecisionAt(int)` | Its decisions. |
| `appWindow().getUserCreatedPDU().addSimplePdu(src: QString, dst: QString)` | Returns `ADD_PDU_ERROR`, `0` when sent. |
