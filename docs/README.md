# pktctl documentation

| Document | Read it when |
|---|---|
| [architecture.md](architecture.md) | You want the big picture: layers, data flow, why PTMP. |
| [features/](features/README.md) | You want to know what each tool does, or how to set up the ExApp. |
| [reference/ptmp.md](reference/ptmp.md) | You need the wire format, message types or value codes. |
| [development.md](development.md) | You are changing code: commands, tests, live E2E, git flow, releases. |
| [../CHANGELOG.md](../CHANGELOG.md) | You want to know what changed between versions. |

Crate level documentation:

- [crates/ptmp](../crates/ptmp/README.md): the protocol client.
- [crates/pktfile](../crates/pktfile/README.md): the `.pkt` codec and XML editor.
- [crates/pktctl](../crates/pktctl/README.md): the MCP server.
