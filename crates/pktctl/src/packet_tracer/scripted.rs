use std::{
    future::Future,
    sync::{Arc, Mutex, PoisonError},
};

use ptmp::{Call, Event, Subscription, Value};
use tokio::sync::broadcast;

use super::{Events, PacketTracer, PtError};
use crate::testing::Canvas;

type Script = dyn Fn(&Call, &Emitter) -> Result<Value, PtError> + Send + Sync;

pub(crate) struct ScriptedPacketTracer {
    version: Result<String, PtError>,
    script: Box<Script>,
    emitter: Emitter,
    subscriptions: Mutex<Vec<Subscription>>,
}

#[derive(Clone)]
pub(crate) struct Emitter(broadcast::Sender<Event>);

impl Emitter {
    pub(crate) fn emit(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

impl ScriptedPacketTracer {
    pub(crate) fn new(
        script: impl Fn(&Call) -> Result<Value, PtError> + Send + Sync + 'static,
    ) -> Self {
        Self::with_events(move |call, _| script(call))
    }

    pub(crate) fn with_events(
        script: impl Fn(&Call, &Emitter) -> Result<Value, PtError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            version: Ok("9.0.1.0858".into()),
            script: Box::new(script),
            emitter: Emitter(broadcast::channel(256).0),
            subscriptions: Mutex::default(),
        }
    }

    pub(crate) fn on_canvas(canvas: Arc<Canvas>) -> Self {
        Self::with_events(move |call, emitter| {
            let reply = canvas.handle(call).map_err(PtError::from);
            for event in canvas.take_events() {
                emitter.emit(event);
            }
            reply
        })
    }

    pub(crate) fn unreachable() -> Self {
        let error = PtError::Unreachable("connection refused".into());
        let mut packet_tracer = Self::new({
            let error = error.clone();
            move |_| Err(error.clone())
        });
        packet_tracer.version = Err(error);
        packet_tracer
    }

    pub(crate) fn emit(&self, event: Event) {
        self.emitter.emit(event);
    }

    pub(crate) fn subscriptions(&self) -> Vec<Subscription> {
        self.subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, subscription: Subscription) {
        self.subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(subscription);
    }
}

impl PacketTracer for ScriptedPacketTracer {
    fn call(&self, call: Call) -> impl Future<Output = Result<Value, PtError>> + Send {
        std::future::ready((self.script)(&call, &self.emitter))
    }

    fn version(&self) -> impl Future<Output = Result<String, PtError>> + Send {
        std::future::ready(self.version.clone())
    }

    fn subscribe(
        &self,
        subscription: Subscription,
    ) -> impl Future<Output = Result<Events, PtError>> + Send {
        let events = self.emitter.0.subscribe();
        self.record(subscription);
        std::future::ready(Ok(events))
    }

    fn unsubscribe(
        &self,
        subscription: Subscription,
    ) -> impl Future<Output = Result<(), PtError>> + Send {
        self.record(Subscription {
            enabled: false,
            ..subscription
        });
        std::future::ready(Ok(()))
    }
}

pub(crate) fn methods(call: &Call) -> Vec<&str> {
    call.steps()
        .iter()
        .map(|step| step.method.as_str())
        .collect()
}
