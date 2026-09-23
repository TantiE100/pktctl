# pktfile

Reads and writes Cisco Packet Tracer `.pkt` files. A `.pkt` file is XML wrapped
in four layers, undone in this order by `decode` and applied in reverse by
`encode`:

1. **Byte scrambling.** The file is reversed and each byte is XOR-ed with
   `(len - i * len) & 0xFF`.
2. **Authenticated encryption.** Twofish in EAX mode with a fixed key
   (`0x89` × 16) and nonce (`0x10` × 16); the 16-byte tag is the last 16 bytes.
3. **Masking.** Each byte is XOR-ed with `(len - i) & 0xFF`.
4. **Qt compression.** A 4-byte big-endian length, then a zlib stream.

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = pktfile::decode(&std::fs::read("lab.pkt")?)?;
    std::fs::write("lab-copy.pkt", pktfile::encode(&xml)?)?;
    Ok(())
}
```

A wrong or damaged file fails the EAX integrity check (`PktError::Integrity`)
instead of producing garbage.

To look inside a file, and to pack it back after editing the XML:

```sh
cargo run -p pktfile --example pkt2xml -- lab.pkt > lab.xml
cargo run -p pktfile --example xml2pkt -- lab.xml lab.pkt
```

## Physical workspace edits

Packet Tracer's IPC API cannot rename, create or delete some physical
locations, so pktctl edits the XML instead. Every edit touches only the
byte ranges of the node it changes:

| Function | Edit |
|---|---|
| `physical_nodes` | Lists every `<NODE>` of `PHYSICALWORKSPACE` with its `UUID_STR`, parent, kind and position. |
| `rename_node` | Replaces a node's `NAME` text. |
| `add_building` | Inserts a building on Packet Tracer's building backdrop, with the size and scale its own toolbar gives one. |
| `remove_node` | Cuts a node out with its children; refuses Intercity and any node that still holds a device (`TYPE` 6), since devices also live in the logical topology. |

`add_node` writes the same fields Packet Tracer writes for a node its own
toolbar creates; nothing is copied out of a Packet Tracer file, and this crate
ships none. The unit tests build their own workspace XML. To exercise the codec
against a real file, point `PKTCTL_TEST_PKT` at any `.pkt` and run the ignored
tests in `tests/real_files.rs`.

The format was documented by [Unpacket](https://github.com/Punkcake21/Unpacket)
(MIT). Files written by `encode` open in Packet Tracer 9.0.1; see the live
tests in `crates/pktctl`.
