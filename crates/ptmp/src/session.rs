use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use futures::{SinkExt, StreamExt, stream::SplitStream};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot},
    time::timeout,
};
use tokio_util::codec::Framed;

use crate::{
    auth::md5_digest,
    call::Call,
    error::Error,
    event::{Event, Subscription},
    frame::FrameCodec,
    message::Message,
    negotiation::Negotiation,
    timestamp,
    value::Value,
};

const EVENT_BUFFER: usize = 256;

type Transport = Framed<TcpStream, FrameCodec>;
type PendingCalls = Mutex<HashMap<u32, oneshot::Sender<Result<Value, Error>>>>;

#[derive(Clone)]
pub struct Credentials {
    pub app_id: String,
    pub secret: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("app_id", &self.app_id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub addr: String,
    pub credentials: Credentials,
    pub connect_timeout: Duration,
    pub call_timeout: Duration,
}

impl SessionConfig {
    pub fn new(addr: impl Into<String>, credentials: Credentials) -> Self {
        Self {
            addr: addr.into(),
            credentials,
            connect_timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    outbound: mpsc::UnboundedSender<Message>,
    pending: PendingCalls,
    next_id: AtomicU32,
    events: broadcast::Sender<Event>,
    closed: AtomicBool,
    pt_version: Option<String>,
    call_timeout: Duration,
}

impl Session {
    pub async fn connect(config: &SessionConfig) -> Result<Self, Error> {
        let connect_error = |source| Error::Connect {
            addr: config.addr.clone(),
            source,
        };
        let stream = timeout(config.connect_timeout, TcpStream::connect(&config.addr))
            .await
            .map_err(|_| connect_error(std::io::ErrorKind::TimedOut.into()))?
            .map_err(connect_error)?;
        stream.set_nodelay(true).map_err(connect_error)?;

        let mut transport = Framed::new(stream, FrameCodec);
        let negotiation = timeout(
            config.connect_timeout,
            handshake(&mut transport, &config.credentials),
        )
        .await
        .map_err(|_| Error::Timeout(config.connect_timeout))??;

        Ok(Self::start(transport, &negotiation, config.call_timeout))
    }

    pub fn pt_version(&self) -> Option<&str> {
        self.shared.pt_version.as_deref()
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    pub async fn call(&self, call: Call) -> Result<Value, Error> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, response) = oneshot::channel();
        self.shared.pending().insert(id, reply);

        if self.is_closed()
            || self
                .shared
                .outbound
                .send(Message::IpcCall { id, call })
                .is_err()
        {
            self.shared.pending().remove(&id);
            return Err(Error::Closed);
        }

        match timeout(self.shared.call_timeout, response).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Closed),
            Err(_) => {
                self.shared.pending().remove(&id);
                Err(Error::Timeout(self.shared.call_timeout))
            }
        }
    }

    pub fn subscribe(&self, subscription: Subscription) -> Result<(), Error> {
        self.shared
            .outbound
            .send(Message::IpcSubscribe(subscription))
            .map_err(|_| Error::Closed)
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    pub fn close(&self) {
        let _ = self.shared.outbound.send(Message::Disconnect {
            reason: String::new(),
        });
        self.shared.closed.store(true, Ordering::Release);
    }

    fn start(transport: Transport, negotiation: &Negotiation, call_timeout: Duration) -> Self {
        let (outbound, outbound_queue) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            outbound,
            pending: Mutex::default(),
            next_id: AtomicU32::new(1),
            events: broadcast::channel(EVENT_BUFFER).0,
            closed: AtomicBool::new(false),
            pt_version: negotiation.pt_version().map(str::to_owned),
            call_timeout,
        });

        let (sink, stream) = transport.split();
        tokio::spawn(write_loop(sink, outbound_queue, Arc::clone(&shared)));
        tokio::spawn(read_loop(stream, Arc::clone(&shared)));
        Self { shared }
    }
}

impl Shared {
    fn pending(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<u32, oneshot::Sender<Result<Value, Error>>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn resolve(&self, id: u32, result: Result<Value, Error>) {
        let Some(reply) = self.pending().remove(&id) else {
            tracing::debug!(id, "reply for a call that already timed out");
            return;
        };
        let _ = reply.send(result);
    }

    fn shut_down(&self) {
        self.closed.store(true, Ordering::Release);
        for (_, reply) in self.pending().drain() {
            let _ = reply.send(Err(Error::Closed));
        }
    }
}

async fn handshake(
    transport: &mut Transport,
    credentials: &Credentials,
) -> Result<Negotiation, Error> {
    let request = Negotiation::client_request(
        format!("{{{}}}", uuid::Uuid::new_v4()),
        timestamp::now_utc(),
    );
    send(transport, Message::NegotiationRequest(request)).await?;
    let negotiation = match receive(transport).await? {
        Message::NegotiationResponse(negotiation) => negotiation,
        other => return Err(unexpected("negotiation response", &other)),
    };
    if let Some(problem) = negotiation.unsupported_setting() {
        return Err(Error::Negotiation(problem));
    }

    send(
        transport,
        Message::AuthRequest {
            app_id: credentials.app_id.clone(),
        },
    )
    .await?;
    let challenge = match receive(transport).await? {
        Message::AuthChallenge { challenge } => challenge,
        other => return Err(unexpected("authentication challenge", &other)),
    };

    send(
        transport,
        Message::AuthResponse {
            app_id: credentials.app_id.clone(),
            digest: md5_digest(&challenge, &credentials.secret),
        },
    )
    .await?;
    match receive(transport).await? {
        Message::AuthStatus { accepted: true } => Ok(negotiation),
        Message::AuthStatus { accepted: false } | Message::Disconnect { .. } => {
            Err(Error::AuthRejected {
                app_id: credentials.app_id.clone(),
            })
        }
        other => Err(unexpected("authentication status", &other)),
    }
}

async fn send(transport: &mut Transport, message: Message) -> Result<(), Error> {
    transport.send(message.to_frame()?).await?;
    Ok(())
}

async fn receive(transport: &mut Transport) -> Result<Message, Error> {
    match transport.next().await {
        Some(frame) => Ok(Message::from_frame(&frame?)?),
        None => Err(Error::Closed),
    }
}

fn unexpected(expected: &'static str, received: &Message) -> Error {
    Error::Unexpected {
        expected,
        received: format!("{received:?}"),
    }
}

async fn write_loop(
    mut sink: futures::stream::SplitSink<Transport, crate::frame::Frame>,
    mut queue: mpsc::UnboundedReceiver<Message>,
    shared: Arc<Shared>,
) {
    while let Some(message) = queue.recv().await {
        let is_disconnect = matches!(message, Message::Disconnect { .. });
        let frame = match message.to_frame() {
            Ok(frame) => frame,
            Err(error) => {
                if let Message::IpcCall { id, .. } = message {
                    shared.resolve(id, Err(error.into()));
                }
                continue;
            }
        };
        if let Err(error) = sink.send(frame).await {
            tracing::warn!(%error, "writing to Packet Tracer failed");
            break;
        }
        if is_disconnect {
            break;
        }
    }
    shared.shut_down();
}

async fn read_loop(mut stream: SplitStream<Transport>, shared: Arc<Shared>) {
    while let Some(frame) = stream.next().await {
        let message = match frame
            .map_err(Error::from)
            .and_then(|frame| Ok(Message::from_frame(&frame)?))
        {
            Ok(message) => message,
            Err(Error::Protocol(error)) => {
                tracing::warn!(%error, "ignoring message pktctl cannot parse");
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "reading from Packet Tracer failed");
                break;
            }
        };
        match message {
            Message::IpcResponse { id, value } => shared.resolve(id, Ok(value)),
            Message::IpcError { id, class, message } => {
                shared.resolve(id, Err(Error::Remote { class, message }));
            }
            Message::IpcEvent(event) => {
                let _ = shared.events.send(event);
            }
            Message::Disconnect { .. } => break,
            Message::KeepAlive => {
                let _ = shared.outbound.send(Message::KeepAlive);
            }
            other => tracing::debug!(message = ?other, "ignoring unsolicited message"),
        }
    }
    shared.shut_down();
}
