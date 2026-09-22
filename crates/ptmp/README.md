# ptmp

Async Rust client for PTMP, the protocol Cisco Packet Tracer exposes to
external applications, and for the IPC layer that runs on top of it.

```rust,no_run
use ptmp::{Call, Credentials, Session, SessionConfig, Value};

# async fn demo() -> Result<(), ptmp::Error> {
let config = SessionConfig::new(
    "127.0.0.1:39000",
    Credentials { app_id: "dev.pktctl".into(), secret: "…".into() },
);
let session = Session::connect(&config).await?;

let model = session
    .call(
        Call::root("network")
            .method("getDevice", [Value::qstring("R1")])
            .method("getModel", []),
    )
    .await?;
assert_eq!(model.as_str(), Some("2911"));
# Ok(()) }
```

## Modules

| Module | Responsibility |
|---|---|
| `frame` | `FrameCodec`: `<length>\0<fields…>` framing for tokio `Framed`. |
| `value` | `Value` and `TypeCode`: the typed values IPC calls send and return. |
| `call` | `Call`: builder for the method path of an IPC call. |
| `event` | `Event` and `Subscription`: pushed IPC events. |
| `negotiation` | Connection negotiation, including the Packet Tracer version. |
| `message` | `Message`: every PTMP message type, to and from frames. |
| `auth` | `md5_digest`: the challenge response. |
| `session` | `Session`: handshake, pipelined calls, events, close. |
| `fake` | `FakePt` (feature `fake`): a PTMP server for tests. |

## Session behaviour

- `connect` negotiates text encoding without encryption or compression and
  authenticates with MD5. Anything else Packet Tracer proposes is refused with
  `Error::Negotiation`.
- `call` can be used concurrently from many tasks; replies are routed by call
  id. Each call has its own timeout (`SessionConfig::call_timeout`).
- When the connection drops, every in-flight call fails with `Error::Closed`
  and `is_closed` turns true. Reconnecting is the caller's decision.
- `events()` returns a broadcast receiver; `subscribe` asks Packet Tracer to
  start pushing an event for one object.

## Errors

| Variant | Meaning |
|---|---|
| `Connect` | TCP connection failed; Packet Tracer is closed or on another port. |
| `AuthRejected` | App id not registered, or wrong secret. |
| `Remote` | Packet Tracer refused the call (`class`, `message`). |
| `Timeout` | No reply within the configured time. |
| `Closed` | The session is gone. |
| `Frame`, `Protocol`, `Negotiation`, `Unexpected` | The peer did not speak PTMP as expected. |

## Testing with `FakePt`

```rust,ignore
let pt = FakePt::start(credentials, |call| Value::Int(11).into()).await?;
let session = Session::connect(&SessionConfig::new(pt.addr().to_string(), credentials)).await?;
```

`FakePt` performs the real handshake over TCP, answers calls with your closure,
records calls and subscriptions, and can `emit` events.

The wire format is documented in [docs/reference/ptmp.md](../../docs/reference/ptmp.md).
