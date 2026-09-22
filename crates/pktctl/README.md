# pktctl

The MCP server binary. It reads its configuration from the environment, serves
MCP over stdio and talks to Packet Tracer through [`ptmp`](../ptmp/README.md).

## Modules

| Module | Responsibility |
|---|---|
| `config` | Reads `PKTCTL_*` variables into `Config`; fails fast with a clear message. |
| `packet_tracer` | The domain port: `PacketTracer` trait, `LivePacketTracer`, `PtError`. |
| `features` | One folder per tool; see [docs/features](../../docs/features/README.md). |
| `server` | `PktctlServer`: composes the feature routers and serves stdio. |

## Configuration

| Variable | Required | Default |
|---|---|---|
| `PKTCTL_APP_ID` | yes | |
| `PKTCTL_SECRET` | yes | |
| `PKTCTL_ADDR` | no | `127.0.0.1:39000` |
| `PKTCTL_CALL_TIMEOUT_SECS` | no | `30` |
| `PKTCTL_LOG` | no | `warn` (stderr) |

`pktctl --version` prints the version and exits.

## Connection lifecycle

`LivePacketTracer` connects on the first tool call, not at startup, so the
server starts even when Packet Tracer is closed and `status` can explain what
is missing. When the session drops, the next call reconnects. A call that was
in flight when the connection dropped is not retried, because it may already
have changed the network.

## Errors seen by the agent

Tool failures are returned as MCP tool errors (`isError: true`) with an
actionable message:

| `PtError` | Typical message |
|---|---|
| `Unreachable` | open Packet Tracer and retry |
| `NotRegistered` | register the ExApp and check the shared secret |
| `Rejected` | Packet Tracer's own reason, for example an unknown device |
| `InvalidInput` | the tool arguments were unusable |
| `UnexpectedReply`, `Transport` | protocol level problems worth a bug report |
