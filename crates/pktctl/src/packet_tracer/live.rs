use ptmp::{Call, Session, SessionConfig, Subscription, Value};
use tokio::sync::Mutex;

use super::{Events, PacketTracer, PtError};

#[derive(Debug)]
pub struct LivePacketTracer {
    config: SessionConfig,
    session: Mutex<Option<Session>>,
}

impl LivePacketTracer {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            session: Mutex::new(None),
        }
    }

    async fn session(&self) -> Result<Session, PtError> {
        let mut current = self.session.lock().await;
        if let Some(session) = current.as_ref().filter(|session| !session.is_closed()) {
            return Ok(session.clone());
        }
        let session = Session::connect(&self.config).await?;
        tracing::info!(version = session.pt_version(), "connected to Packet Tracer");
        *current = Some(session.clone());
        Ok(session)
    }
}

impl PacketTracer for LivePacketTracer {
    async fn call(&self, call: Call) -> Result<Value, PtError> {
        Ok(self.session().await?.call(call).await?)
    }

    async fn version(&self) -> Result<String, PtError> {
        let session = self.session().await?;
        Ok(session.pt_version().unwrap_or("unknown").to_owned())
    }

    async fn subscribe(&self, subscription: Subscription) -> Result<Events, PtError> {
        let session = self.session().await?;
        let events = session.events();
        session.subscribe(subscription)?;
        Ok(events)
    }

    async fn unsubscribe(&self, subscription: Subscription) -> Result<(), PtError> {
        let session = self.session().await?;
        Ok(session.subscribe(Subscription {
            enabled: false,
            ..subscription
        })?)
    }
}
