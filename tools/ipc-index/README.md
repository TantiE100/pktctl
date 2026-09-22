# ipc-index

Generates `crates/pktctl/assets/ipc-index.json`, the map of Packet Tracer's IPC
API that `describe_ipc` and `call_ipc` use.

```bash
IPC="/Applications/Cisco Packet Tracer 9.0.1/Cisco Packet Tracer 9.0.1.app/Contents/help/default/ipc"
python3 tools/ipc-index/generate.py \
  "$IPC/pt-cep-java-framework-9.0.0.0.jar" \
  crates/pktctl/assets/ipc-index.json \
  "$IPC/pt-cep-java-framework-9.0.0.0-docs.zip"
```

Needs Python 3 and `javap` from any JDK. Rerun it when a new Packet Tracer
release ships a new framework, then run `make check`: the index tests pin
signatures that were verified against a live Packet Tracer.

## What it reads

| Source | Gives |
|---|---|
| Interfaces in `com.cisco.pt.ipc.*` | Classes, inheritance, Java signatures. |
| `*Impl` bytecode | Wire method name (`getObjectUUID` is sent as `getObjectUuid`) and the `IPCCall.add*Parameter` sequence, the exact PTMP type of each argument. |
| `IPCFactory` bytecode | The same for methods that return objects, which the implementations delegate to the factory and its `createMessage` builders. |
| Enum static initialisers | Wire values (`ConnectType.ETHERNET_STRAIGHT = 8100`), which differ from ordinals. |
| `IPCResponseFactory` and each value object's `read` method | Wire class names of value objects (PTMP type 16) and their field names and types, in order. |
| `*EventRegistry` bytecode | Each class's wire name (`getClassName`) and the event names its `processEvent` accepts. |
| Javadoc zip | Parameter names, and method summaries only with `--with-summaries`. |

It prints a summary line. `0 unresolved remote params` means every argument of
every remote method has a known wire type. Classes that do not extend
`IPCObject` (PDU headers, table entries) are data returned by value and are
marked `"remote": false`.

## Javadoc prose is not redistributed

The summaries in the Javadoc are Cisco's text, so the index in this repository
carries none: the generator drops them unless you pass `--with-summaries`. A
local index built with that flag makes `describe_ipc` more informative and is
fine to keep on your machine; do not commit or publish it. Everything else the
index holds, class and method names, wire types, enum values and value-object
layouts, is the API surface `call_ipc` needs to talk to Packet Tracer at all.
