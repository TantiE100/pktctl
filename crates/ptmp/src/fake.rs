//! In-process Packet Tracer stand-in that speaks real PTMP over TCP, for tests.

use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
};

use futures::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast,
    task::JoinHandle,
};
use tokio_util::codec::Framed;

use crate::{
    auth::md5_digest,
    call::Call,
    event::{Event, Subscription},
    frame::FrameCodec,
    message::Message,
    negotiation::Negotiation,
    session::Credentials,
    value::Value,
};

pub const FAKE_PT_VERSION: &str = "9.0.1.0858";
const CHALLENGE: &str = "fakeChallenge0123456789abcdefghi";

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Value(Value),
    Error { class: String, message: String },
    WithEvents { value: Value, events: Vec<Event> },
    Silence,
}

impl Reply {
    pub fn error(class: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            class: class.into(),
            message: message.into(),
        }
    }
}

impl From<Value> for Reply {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

type Handler = dyn Fn(&Call) -> Reply + Send + Sync;

#[derive(Debug)]
pub struct FakePt {
    addr: SocketAddr,
    events: broadcast::Sender<Event>,
    state: Arc<State>,
    server: JoinHandle<()>,
}

struct State {
    credentials: Credentials,
    handler: Box<Handler>,
    calls: Mutex<Vec<Call>>,
    subscriptions: Mutex<Vec<Subscription>>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl FakePt {
    pub async fn start(
        credentials: Credentials,
        handler: impl Fn(&Call) -> Reply + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let events = broadcast::channel(64).0;
        let state = Arc::new(State {
            credentials,
            handler: Box::new(handler),
            calls: Mutex::default(),
            subscriptions: Mutex::default(),
        });

        let server = tokio::spawn({
            let events = events.clone();
            let state = Arc::clone(&state);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, Arc::clone(&state), events.subscribe()));
                }
            }
        });

        Ok(Self {
            addr,
            events,
            state,
            server,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    pub fn calls(&self) -> Vec<Call> {
        lock(&self.state.calls).clone()
    }

    pub fn subscriptions(&self) -> Vec<Subscription> {
        lock(&self.state.subscriptions).clone()
    }
}

impl Drop for FakePt {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn serve(stream: TcpStream, state: Arc<State>, mut emitted: broadcast::Receiver<Event>) {
    let mut transport = Framed::new(stream, FrameCodec);
    if !authenticate(&mut transport, &state.credentials).await {
        return;
    }

    let mut subscribed: HashSet<(String, String, String)> = HashSet::new();
    loop {
        let outgoing = tokio::select! {
            frame = transport.next() => {
                let Some(Ok(frame)) = frame else { return };
                let Ok(message) = Message::from_frame(&frame) else { continue };
                match message {
                    Message::IpcCall { id, call } => {
                        lock(&state.calls).push(call.clone());
                        answer(id, (state.handler)(&call))
                    }
                    Message::IpcSubscribe(subscription) => {
                        let key = subscription_key(&subscription.class, &subscription.object_uuid, &subscription.event);
                        if subscription.enabled {
                            subscribed.insert(key);
                        } else {
                            subscribed.remove(&key);
                        }
                        lock(&state.subscriptions).push(subscription);
                        Vec::new()
                    }
                    Message::Disconnect { .. } => return,
                    _ => Vec::new(),
                }
            }
            event = emitted.recv() => {
                let Ok(event) = event else { continue };
                vec![Message::IpcEvent(event)]
            }
        };

        for message in outgoing {
            if let Message::IpcEvent(event) = &message
                && !subscribed.contains(&subscription_key(
                    &event.class,
                    &event.object_uuid,
                    &event.name,
                ))
            {
                continue;
            }
            if send(&mut transport, &message).await.is_err() {
                return;
            }
        }
    }
}

fn answer(id: u32, reply: Reply) -> Vec<Message> {
    match reply {
        Reply::Value(value) => vec![Message::IpcResponse { id, value }],
        Reply::Error { class, message } => vec![Message::IpcError { id, class, message }],
        Reply::WithEvents { value, events } => std::iter::once(Message::IpcResponse { id, value })
            .chain(events.into_iter().map(Message::IpcEvent))
            .collect(),
        Reply::Silence => Vec::new(),
    }
}

fn subscription_key(class: &str, object_uuid: &str, event: &str) -> (String, String, String) {
    (class.to_owned(), object_uuid.to_owned(), event.to_owned())
}

async fn authenticate(
    transport: &mut Framed<TcpStream, FrameCodec>,
    credentials: &Credentials,
) -> bool {
    let Some(Message::NegotiationRequest(request)) = next(transport).await else {
        return false;
    };
    let response = Negotiation {
        app_uuid: format!("{{{}}}", uuid::Uuid::new_v4()),
        ..request
    }
    .with_pt_version(FAKE_PT_VERSION);
    if send(transport, &Message::NegotiationResponse(response))
        .await
        .is_err()
    {
        return false;
    }

    let Some(Message::AuthRequest { .. }) = next(transport).await else {
        return false;
    };
    let challenge = Message::AuthChallenge {
        challenge: CHALLENGE.into(),
    };
    if send(transport, &challenge).await.is_err() {
        return false;
    }

    let Some(Message::AuthResponse { app_id, digest }) = next(transport).await else {
        return false;
    };
    let accepted =
        app_id == credentials.app_id && digest == md5_digest(CHALLENGE, &credentials.secret);
    let verdict = if accepted {
        Message::AuthStatus { accepted: true }
    } else {
        Message::Disconnect {
            reason: String::new(),
        }
    };
    send(transport, &verdict).await.is_ok() && accepted
}

async fn next(transport: &mut Framed<TcpStream, FrameCodec>) -> Option<Message> {
    let frame = transport.next().await?.ok()?;
    Message::from_frame(&frame).ok()
}

async fn send(
    transport: &mut Framed<TcpStream, FrameCodec>,
    message: &Message,
) -> Result<(), crate::error::FrameError> {
    let frame = message
        .to_frame()
        .expect("fake server only sends encodable messages");
    transport.send(frame).await
}
