# ipc-index

Builds `crates/pktctl/assets/ipc-index.json`, the map of Packet Tracer's IPC API
that `describe_ipc` and `call_ipc` use.

```bash
IPC="/Applications/Cisco Packet Tracer 9.0.1/Cisco Packet Tracer 9.0.1.app/Contents/help/default/ipc"
cargo run -p ipc-index -- \
  "$IPC/pt-cep-java-framework-9.0.0.0.jar" \
  crates/pktctl/assets/ipc-index.json \
  "$IPC/pt-cep-java-framework-9.0.0.0-docs.zip"
```

Nothing else is needed: it reads the class files out of the jar itself, so there
is no JDK and no `javap` in the way. Rerun it when a new Packet Tracer release
ships a new framework, then run `make check`: the index tests pin signatures that
were verified against a live Packet Tracer.

## What it reads

| Source | Gives |
|---|---|
| Interfaces in `com.cisco.pt.ipc.*` | Classes, inheritance, and each method's Java types, from the generic signature when the compiler recorded one. |
| `*Impl` bytecode | Wire method name (`getObjectUUID` is sent as `getObjectUuid`) and the `IPCCall.add*Parameter` sequence, the exact PTMP type of each argument. |
| `IPCFactory` bytecode | The same for methods that return objects, which the implementations delegate to the factory and its `createMessage` builders. |
| Enum class initialisers | Wire values (`ConnectType.ETHERNET_STRAIGHT = 8100`), which differ from ordinals. |
| `IPCResponseFactory` and each value object's `read` method | Wire class names of value objects (PTMP type 16) and their field names and types, in order. |
| `*EventRegistry` bytecode | Each class's wire name (`getClassName`) and the event names its `processEvent` accepts. |
| Javadoc zip | Parameter names, and method summaries only with `--with-summaries`. |

It prints a summary line. `0 unresolved remote params` means every argument of
every remote method has a known wire type. Classes that do not extend
`IPCObject` (PDU headers, table entries) are data returned by value and are
marked `"remote": false`.

## How it reads a class file

| Module | Job |
|---|---|
| `classfile.rs` | The constant pool, what a class extends and implements, and every method with its descriptor, generic signature and code. |
| `code.rs` | Walks the bytecode and reports the instructions the index needs: constants pushed, calls, field stores and backward jumps. Stepping over every instruction shape is what keeps the walk in step with the code. |
| `types.rs` | Descriptors and generic signatures into Java type names. |
| `wire.rs`, `enums.rs`, `layouts.rs`, `events.rs` | One section of the index each. |

## Javadoc prose is not redistributed

The summaries in the Javadoc are Cisco's text, so the index in this repository
carries none: the generator drops them unless you pass `--with-summaries`. A
local index built with that flag makes `describe_ipc` more informative and is
fine to keep on your machine; do not commit or publish it. Everything else the
index holds, class and method names, wire types, enum values and value-object
layouts, is the API surface `call_ipc` needs to talk to Packet Tracer at all.
