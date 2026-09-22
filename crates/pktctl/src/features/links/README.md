# links

Ports and the cables between them.

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `list_ports` | `device` | every port with state, address and peer |
| `list_links` | none | every cable with both ends and its type |
| `connect` | `device_a`, `port_a`, `device_b`, `port_b`, optional `cable` | the new link |
| `disconnect` | `device`, `port` | the removed link |

```json
{ "device_a": "R1", "port_a": "GigabitEthernet0/0", "device_b": "SW1", "port_b": "GigabitEthernet0/1" }
```

```json
{ "a": { "device": "R1", "port": "GigabitEthernet0/0" }, "b": { "device": "SW1", "port": "GigabitEthernet0/1" }, "cable": "straight" }
```

A port from `list_ports`:

```json
{ "name": "FastEthernet0", "up": true, "protocol_up": true, "ip": "192.168.9.10", "mask": "255.255.255.0",
  "connection": { "to": { "device": "Switch0", "port": "FastEthernet0/1" }, "cable": "straight" } }
```

`ip` and `mask` appear only on ports that have an address (switch ports never
do). `up` follows the physical state, so a cabled router port that is still
`shutdown` reports `up: false` on both ends.

## Cables

`cable` accepts `auto` (default), `straight`, `cross`, `rollover`, `fiber`,
`fiber_multimode`, `serial`, `console`, `phone`, `coaxial`, `octal`, `usb`.

Packet Tracer's own automatic cable (`ConnectType.AUTO`) is refused by
`createLink`, so `auto` is decided by pktctl with the CCNA rules:

| Ends | Cable |
|---|---|
| any `Serial` port | serial |
| a console or RS-232 port | rollover |
| host or router to switch, hub, access point, cloud or modem | straight |
| devices of the same layer (switch to switch, router to router, PC to router, PC to PC) | cross |

Fiber-only ports cannot be told apart by name; pass `cable: "fiber"` for them.

## Safety checks

Before creating a link pktctl verifies that both devices and ports exist and
that both ports are free. Otherwise it answers with the reason, for example
`SW1:FastEthernet0/1 is already connected to PC1:FastEthernet0` or
``Router2 has no port `Gi0/1`; its ports are Vlan1, GigabitEthernet0/0, ...``.
When Packet Tracer still refuses (wrong medium, wireless port), the error says
which cable did not fit.

## IPC calls

| Purpose | Call |
|---|---|
| port list | `network().getDevice(d).getPortCount()`, `getPortAt(i: int)` then `getName()`, `isPortUp()`, `isProtocolUp()`, `getIpAddress()`, `getSubnetMask()` |
| peer | `...getLink().getConnectionType()` (missing link answers `Link: IPC Cache entry`), `...getLink().getPort1()` / `getPort2()` then `getOwnerDevice().getName()` and `getName()` |
| all links | `network().getLinkCount()`, `getLinkAt(i: int)` then the same peer calls |
| create | `...getLogicalWorkspace().createLink(dev_a: QString, port_a: string, dev_b: QString, port_b: string, cable: int)` returns bool |
| delete | `...getLogicalWorkspace().deleteLink(device: QString, port: string)` returns bool |

Cable codes are `ConnectType` values: straight 8100, cross 8101, rollover 8102,
fiber 8103, serial 8106, console 8108, fiber multimode 8117.
