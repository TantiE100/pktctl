# ipc

The floor under every other feature: the whole Packet Tracer IPC API, checked
against the official framework, available to the agent.

The dedicated tools cover common work with friendly arguments. Everything else
Packet Tracer can do through IPC, such as simulation mode, the physical
workspace, wireless settings, server services, ACL objects, preferences or
activities, is reachable here without waiting for a dedicated tool.

## Tools

### `describe_ipc`

| Argument | Meaning |
|---|---|
| `search` | Keywords matched against class names, method names and Javadoc summaries, best matches first. |
| `class` | One class with all its methods, inherited ones included, plus its ancestors and subclasses. |
| `enum` | The names and wire values an enum argument accepts. |
| `limit` | Maximum search results (default 40, maximum 200). |

With no argument it returns the roots and totals: 346 classes, 3111 methods and
133 enums for Packet Tracer 9.0.1.

```json
{ "search": "ssid" }
```

```json
{ "matches": [ { "class": "WirelessCommon", "signature": "setSsid(ssid: string) -> void", "doc": "Sets the SSID." } ] }
```

### `call_ipc`

| Argument | Meaning |
|---|---|
| `from` | A root (`network`, `appWindow`, `simulation`, `options`, `hardwareFactory`, `ipcManager`, `multiUserManager`, `userAppManager`, `commandLog`, `systemFileManager`) or the uuid of an object returned earlier. |
| `steps` | Methods called one after another, each on the object the previous one returned. Each step is `{ "method", "args" }`. |

```json
{
  "from": "network",
  "steps": [
    { "method": "getDevice", "args": ["R1"] },
    { "method": "getPort", "args": ["GigabitEthernet0/0"] },
    { "method": "getIpAddress" }
  ]
}
```

```json
{ "call": "network.getDevice(\"R1\").getPort(\"GigabitEthernet0/0\").getIpAddress()", "returns": "ipv4", "value": "192.168.10.1" }
```

- **Arguments** are plain JSON. IP and MAC addresses and uuids go as strings.
  Enums go by name, case-insensitive (`"ethernet_straight"`), or by wire value
  (`8100`). Byte lists go as arrays.
- **Types are never guessed.** Every step is resolved against the index before
  anything is sent: method name, argument count and each argument's exact
  PTMP type (`string` and `QString` are different on the wire). Mistakes come
  back as tool errors with the right signature or the closest method names.
- **Dynamic classes.** `getDevice` is declared to return `Device`, but the
  object may be a `Router`. When a method is not on the declared class, the
  tool asks Packet Tracer for the object's real class (`getClassName`) and
  resolves against that.
- **Objects** cannot travel over PTMP. When the last step returns one, the tool
  replies with `{ "class", "uuid" }`, and a later call can start from that
  uuid through `getObjectByUuid`.
- **Enums** come back as `{ "name", "value" }`. Byte lists come back as
  `{ "bytes", "base64" }`.
- **Value objects** (PTMP type 16: ACL statements, ARP tables, routing
  entries, flowchart nodes, packet headers, 359 classes) come back as JSON
  objects with their field names, for example
  `{ "class": "FlowChartNode", "strID": "...", "description": "...", "isOSIIn": false, "OSILayerNumber": 3 }`.
  The layouts come from each class's `read` method in the framework and are
  registered with `ptmp` at startup, so a value object inside a pair or a
  vector is read exactly. The 16 classes whose `read` loops are read field by
  field until the next token is not a type code.

Methods that only exist inside the Java client (`getFactory`,
`getAccessMessage`, `getPacketTracerSession`) are left out.

## Where the index comes from

`crates/pktctl/assets/ipc-index.json` is generated from the framework jar and
Javadoc that ship with Packet Tracer; see
[tools/ipc-index](../../../../../tools/ipc-index/README.md). The generator reads
each method's wire name and argument encoders from the bytecode of its
implementation, following delegations through `IPCFactory`, so the index
matches what the official Java client sends.
