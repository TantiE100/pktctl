use std::future::Future;

use ptmp::{Call, Value};

use super::{PacketTracer, PtError};

type Script = dyn Fn(&Call) -> Result<Value, PtError> + Send + Sync;

pub(crate) struct ScriptedPacketTracer {
    version: Result<String, PtError>,
    script: Box<Script>,
}

impl ScriptedPacketTracer {
    pub(crate) fn new(
        script: impl Fn(&Call) -> Result<Value, PtError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            version: Ok("9.0.1.0858".into()),
            script: Box::new(script),
        }
    }

    pub(crate) fn unreachable() -> Self {
        let error = PtError::Unreachable("connection refused".into());
        Self {
            version: Err(error.clone()),
            script: Box::new(move |_| Err(error.clone())),
        }
    }
}

impl PacketTracer for ScriptedPacketTracer {
    fn call(&self, call: Call) -> impl Future<Output = Result<Value, PtError>> + Send {
        std::future::ready((self.script)(&call))
    }

    fn version(&self) -> impl Future<Output = Result<String, PtError>> + Send {
        std::future::ready(self.version.clone())
    }
}

pub(crate) fn methods(call: &Call) -> Vec<&str> {
    call.steps()
        .iter()
        .map(|step| step.method.as_str())
        .collect()
}
