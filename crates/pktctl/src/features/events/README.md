# events

Packet Tracer's live IPC events: what happens while an agent, a student or a
script works on the network.

## Tool

`watch_events` listens for a few seconds and returns what it heard.

| Argument | Meaning |
|---|---|
| `class` | Class raising the events: `LogicalWorkspace`, `TerminalLine`, `Simulation`, `Device`, `Port`, `ArpProcess`, ... (73 classes). |
| `events` | Event names; omit for every event of the class. Case-insensitive. |
| `object` | Only events from one object, by the uuid `call_ipc` returns; omit for all objects of the class. |
| `seconds` | Listening window, 1 to 120 (default 10). |
| `max_events` | Stop early after this many (default 100, maximum 1000). |

```json
{ "class": "LogicalWorkspace", "events": ["deviceAdded", "linkCreated"], "seconds": 8 }
```

```json
{
  "events": [
    { "class": "LogicalWorkspace", "event": "deviceAdded", "object": "{cd80...}",
      "args": ["Router5", "2911", "{ca94...}"] },
    { "class": "LogicalWorkspace", "event": "linkCreated", "object": "{cd80...}",
      "args": ["EV-R1", "GigabitEthernet0/0", "EV-S1", "GigabitEthernet0/1", 8100] }
  ],
  "truncated": false
}
```

`describe_ipc` with `{ "events": "" }` lists every class and its events, or
`{ "events": "Simulation" }` one class. The names come from each event
registry's `processEvent` in the official framework, so they are exactly the
names Packet Tracer sends.

The tool call blocks while it listens. An MCP client that runs tool calls in
parallel can watch while another call acts; otherwise start the watch and do
the action in Packet Tracer by hand.

## Things worth knowing

- Subscribing with an empty object uuid delivers the event for every object of
  the class; verified live.
- `deviceAdded` reports the name the device was created with (`Router5`); the
  rename that `add_device` performs afterwards arrives separately.
- Arguments are decoded like `call_ipc` results: addresses as strings, value
  objects as named JSON objects.

## IPC messages

Subscription is PTMP message type 104 `(class, object uuid, event, bool)`,
once per event name to start and again with `false` to stop. Events arrive as
type 103 and are routed to every listener of the session.
