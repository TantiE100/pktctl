# PTMP wire reference

PTMP (Packet Tracer Messaging Protocol) is the TCP protocol Packet Tracer uses
for IPC with external applications (default port 39000) and for multi-user
(port 38000). Cisco publishes the transport part as the
[PTMP Specification Document](https://tutorials.ptnetacad.net/help/default/files/CiscoPacketTracerPTMPSpecification.pdf).
The IPC payloads (message types 100 to 104) and the value codes are not in that
document; the sections marked **observed** below were derived from the official
Java framework and confirmed against a live Packet Tracer 9.0.1.

## Framing

Every message is `<length>\0<body>`:

- `length` is the body size in **bytes** as ASCII decimal. The specification
  says "bytes or characters"; with text encoding it is bytes of UTF-8
  (**observed**: a device renamed to `Oficiña-Ñandú-→` round-trips).
- `body` is a list of fields, each terminated by `\0`. The first field is the
  message type. The one exception is a byte list value (see below): its bytes
  are sent raw, without terminator, so a body is not guaranteed to end in `\0`
  and must be parsed as bytes, not split into strings up front.

```
7\0 5\0 true\0          -> type 5 (auth status), field "true"
```

pktctl negotiates text encoding, no encryption and no compression, so frames
are plain text on the wire.

## Message types

| Type | Message | Fields after the type |
|---|---|---|
| 0 | Negotiation request | see below |
| 1 | Negotiation response | see below |
| 2 | Auth request | app id |
| 3 | Auth challenge | 32-character challenge |
| 4 | Auth response | app id, digest, custom (empty) |
| 5 | Auth status | `true` / `false` |
| 6 | Keep-alive | none |
| 7 | Disconnect | reason (may be absent) |
| 100 | IPC call **observed** | call id, call path |
| 101 | IPC error **observed** | call id, class, message |
| 102 | IPC response **observed** | call id, optional typed value |
| 103 | IPC event **observed** | token, class, object uuid, event name, typed args, `0` |
| 104 | IPC event subscription **observed** | class, object uuid, event name, `true` / `false` |

## Handshake

```
client → 0  PTMP 1 {client-uuid} 1 1 1 4 20260922003430 0 ""
server → 1  PTMP 1 {server-uuid} 1 1 1 4 20260922003430 0 :PTVER9.0.1.0858
client → 2  dev.pktctl
server → 3  23k4SQ42tTFM6u4deW2jb1F2dZltz4t7
client → 4  dev.pktctl 3417B8057B803EBC150BC7DABA451340 ""
server → 5  true
```

- Negotiation fields: signature `PTMP`, protocol version `1`, app uuid,
  encoding (1 text), encryption (1 none), compression (1 none), authentication
  (4 MD5), timestamp `YYYYMMDDHHMMSS`, keep-alive seconds (0 disables them),
  reserved. The server puts `:PTVER<version>` in the reserved field.
- Digest: uppercase hex of `MD5(challenge + secret)`.
- The app id must belong to a registered ExApp and the secret must be its KEY.
  Otherwise Packet Tracer answers the auth response with a disconnect (type 7).

## Calls (type 100)

A call is a path of method invocations starting at the root `IPC` object,
resolved from scratch on every call. There are no object handles.

```
100  4  network 0  getDevice 9 R1 0  getPort 8 GigabitEthernet0/0 0  getIpAddress 0
```

Each step is `method`, then `type value` pairs for its arguments, then `0`.
The reply is matched by call id, so many calls can be in flight at once.

```
102  4  10 192.168.0.1                         value of type 10 (IP address)
102  9                                         void
101  2  Device  IPC Cache entry:               error: device not found
101  3  Network IPC call "noSuchMethod" not found
```

## Value codes

| Code | Type | Text form |
|---|---|---|
| 0 | void | nothing; also terminates argument lists |
| 1 | byte | decimal |
| 2 | bool | `true` / `false` |
| 3 | short | decimal |
| 4 | int | decimal |
| 5 | long | decimal |
| 6 | float | decimal |
| 7 | double | decimal |
| 8 | string | text |
| 9 | QString | text |
| 10 | IPv4 address | `x.x.x.x` |
| 11 | IPv6 address | `x:x:x:x:x:x:x:x` |
| 12 | MAC address | `xxxx.xxxx.xxxx` |
| 13 | UUID | `{8-4-4-4-12}` |
| 14 | pair **observed** | two typed values |
| 15 | vector **observed** | element type, count, then `count` values without per-item codes |
| 15 + 1 | byte list **observed** | `15 1 <count>` then `count` raw bytes, no terminators (for example a PNG screenshot) |
| 16 | value object **observed** | class name on the wire (`FlowChartNode`, `ArpProcess`-style names may differ in case from the Java interface), then its fields, each as a typed value, with no count. The field layout is the sequence of `read*` calls in the class's `read(EncodedBuffer)` method; `ptmp::data::register` takes those counts. Inside value objects Packet Tracer also sends single-token fields as type 16: an IPv4, IPv6 or MAC address (`16 192.168.10.5`), or a plain string (`16 www.gamc.bo`) when the token names no registered class |

String and QString are the same text on the wire, but each method accepts
exactly one of them. Sending `getPort 9 ...` instead of `getPort 8 ...` fails
with `Invalid arguments for IPC call "getPort"`.

## Finding the exact signature

Packet Tracer ships the official framework inside its installation:

```
<Packet Tracer>/help/default/ipc/pt-cep-java-framework-<version>.jar
<Packet Tracer>/help/default/ipc/pt-cep-java-framework-<version>-docs.zip
```

- Method names and Java types: `javap -cp <jar> com.cisco.pt.ipc.sim.Device`.
- String flavour of each argument: the `*Impl` class delegates to
  `com.cisco.pt.ipc.IPCFactory`, whose bytecode calls either
  `createStringParameterMessage` (8) or `createQStringParameterMessage` (9).
- Enum integers: `javap -c` on the enum. Each constant is built as
  `(name, ordinal, value)` and the wire uses the **value**, which is not always
  the ordinal. `DeviceType` values match their ordinals (`ROUTER=0`, `SWITCH=1`,
  `ACCESS_POINT=7`, `PC=8`), but `ConnectType` starts at 8100
  (`ETHERNET_STRAIGHT=8100`, `ETHERNET_CROSS=8101`, `SERIAL=8106`, `AUTO=8107`).
- Semantics (valid modes, return meanings): the Javadoc, which quotes the
  original `.pki` declarations.

## Events

Subscribe with type 104 using the object uuid (`getObjectUuid` on the object).
Packet Tracer then pushes type 103 messages:

```
104  Device {5938e156-...} nameChanged true
103  1465458924 Device {5938e156-...} nameChanged 9 R1X 9 R1 0
```

The meaning of the leading token is not documented; pktctl passes it through.
