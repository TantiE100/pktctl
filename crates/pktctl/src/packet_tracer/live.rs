use ptmp::{Call, Session, SessionConfig, Subscription, Value};
use tokio::sync::Mutex;

use super::{Events, PacketTracer, PtError};

#[derive(Debug)]
pub struct LivePacketTracer {
    config: SessionConfig,
    addresses: Vec<String>,
    session: Mutex<Option<Session>>,
    connected: std::sync::Mutex<Option<String>>,
}

impl LivePacketTracer {
    /// Connects on first use to the first of `addresses` where Packet Tracer accepts
    /// the credentials; an empty list means `config.addr` alone.
    pub fn new(mut config: SessionConfig, addresses: Vec<String>) -> Self {
        if config.data_layouts.is_empty() {
            config.data_layouts = std::sync::Arc::new(super::api::ApiIndex::data_layouts());
        }
        let addresses = if addresses.is_empty() {
            vec![config.addr.clone()]
        } else {
            addresses
        };
        Self {
            config,
            addresses,
            session: Mutex::new(None),
            connected: std::sync::Mutex::new(None),
        }
    }

    async fn session(&self) -> Result<Session, PtError> {
        let mut current = self.session.lock().await;
        if let Some(session) = current.as_ref().filter(|session| !session.is_closed()) {
            return Ok(session.clone());
        }
        let session = self.connect().await?;
        *current = Some(session.clone());
        Ok(session)
    }

    /// Tries the address that worked last, then the others in order. A port only counts
    /// when the PTMP handshake with our credentials completes there, so another program
    /// on a nearby port is skipped. When nothing answers, the most telling failure is
    /// reported: a rejected app id before a busy Packet Tracer before a closed port.
    async fn connect(&self) -> Result<Session, PtError> {
        let last = self.connected_address();
        let order = last.iter().chain(
            self.addresses
                .iter()
                .filter(|addr| Some(*addr) != last.as_ref()),
        );
        let mut failure: Option<PtError> = None;
        for addr in order {
            let config = SessionConfig {
                addr: addr.clone(),
                ..self.config.clone()
            };
            match Session::connect(&config).await {
                Ok(session) => {
                    tracing::info!(
                        addr,
                        version = session.pt_version(),
                        "connected to Packet Tracer"
                    );
                    *self
                        .connected
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(addr.clone());
                    return Ok(session);
                }
                Err(error) => {
                    let error = PtError::from(error);
                    tracing::debug!(addr, %error, "no Packet Tracer here");
                    if failure
                        .as_ref()
                        .is_none_or(|kept| rank(&error) < rank(kept))
                    {
                        failure = Some(error);
                    }
                }
            }
        }
        Err(match failure {
            Some(PtError::Unreachable(_)) if self.addresses.len() > 1 => {
                PtError::Unreachable(format!(
                    "nothing answered on {} to {}",
                    self.addresses[0],
                    self.addresses[self.addresses.len() - 1]
                ))
            }
            Some(error) => error,
            None => PtError::Unreachable("no address to connect to".into()),
        })
    }

    fn connected_address(&self) -> Option<String> {
        self.connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn rank(error: &PtError) -> u8 {
    match error {
        PtError::NotRegistered(_) => 0,
        PtError::Busy(_) => 1,
        PtError::Unreachable(_) => 3,
        _ => 2,
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

    fn address(&self) -> Option<String> {
        self.connected_address()
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

#[cfg(test)]
mod tests {
    use ptmp::{
        Credentials,
        fake::{FakePt, Reply},
    };

    use super::*;

    fn credentials(secret: &str) -> Credentials {
        Credentials {
            app_id: "dev.pktctl.test".into(),
            secret: secret.into(),
        }
    }

    async fn packet_tracer(secret: &str) -> FakePt {
        FakePt::start(credentials(secret), |_| Reply::Value(Value::Int(1)))
            .await
            .unwrap()
    }

    async fn closed_port() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().to_string()
    }

    fn live(addresses: Vec<String>) -> LivePacketTracer {
        LivePacketTracer::new(
            SessionConfig::new(addresses[0].clone(), credentials("right")),
            addresses,
        )
    }

    #[tokio::test]
    async fn skips_closed_ports_and_strangers_until_packet_tracer_answers() {
        let stranger = packet_tracer("wrong").await;
        let ours = packet_tracer("right").await;
        let pt = live(vec![
            closed_port().await,
            stranger.addr().to_string(),
            ours.addr().to_string(),
        ]);
        assert_eq!(pt.address(), None);
        assert_eq!(pt.call(Call::root("network")).await.unwrap(), Value::Int(1));
        assert_eq!(pt.address(), Some(ours.addr().to_string()));
    }

    #[tokio::test]
    async fn moves_on_from_a_port_that_accepts_but_never_speaks_ptmp() {
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let silent_addr = silent.local_addr().unwrap().to_string();
        let _held = tokio::spawn(async move {
            let mut open = Vec::new();
            while let Ok((stream, _)) = silent.accept().await {
                open.push(stream);
            }
        });
        let ours = packet_tracer("right").await;
        let mut config = SessionConfig::new(silent_addr.clone(), credentials("right"));
        config.connect_timeout = std::time::Duration::from_millis(200);
        let pt = LivePacketTracer::new(config, vec![silent_addr, ours.addr().to_string()]);
        assert_eq!(pt.version().await.unwrap(), ptmp::fake::FAKE_PT_VERSION);
        assert_eq!(pt.address(), Some(ours.addr().to_string()));
    }

    #[tokio::test]
    async fn reports_the_most_telling_failure() {
        let stranger = packet_tracer("wrong").await;
        let pt = live(vec![closed_port().await, stranger.addr().to_string()]);
        let error = pt.version().await.unwrap_err();
        assert!(matches!(error, PtError::NotRegistered(_)), "{error}");

        let nobody = live(vec![closed_port().await, closed_port().await]);
        let error = nobody.version().await.unwrap_err();
        assert!(matches!(error, PtError::Unreachable(_)), "{error}");
        assert!(error.to_string().contains("nothing answered on"), "{error}");
    }
}
