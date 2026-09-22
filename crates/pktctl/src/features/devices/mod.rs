use futures::future::try_join_all;
use ptmp::{Call, Value};
use rmcp::{Json, tool, tool_router};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    packet_tracer::{PacketTracer, PtError, expect_integer, expect_text},
    server::PktctlServer,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Device {
    pub name: String,
    pub model: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DeviceList {
    pub devices: Vec<Device>,
}

pub async fn list<P: PacketTracer>(packet_tracer: &P) -> Result<DeviceList, PtError> {
    let count = packet_tracer
        .call(Call::root("network").method("getDeviceCount", []))
        .await?;
    let count = expect_integer(&count, "device count")?;
    let count = i32::try_from(count)
        .map_err(|_| PtError::UnexpectedReply(format!("device count {count} is out of range")))?;
    let devices = try_join_all((0..count).map(|index| describe(packet_tracer, index))).await?;
    Ok(DeviceList { devices })
}

async fn describe<P: PacketTracer>(packet_tracer: &P, index: i32) -> Result<Device, PtError> {
    let read = |method: &'static str| async move {
        let call = Call::root("network")
            .method("getDeviceAt", [Value::Int(index)])
            .method(method, []);
        expect_text(&packet_tracer.call(call).await?, method)
    };
    let (name, model, kind) =
        tokio::try_join!(read("getName"), read("getModel"), read("getClassName"))?;
    Ok(Device { name, model, kind })
}

#[tool_router(router = devices_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_devices",
        description = "List every device in the open network with its model and kind \
                       (Router, CiscoDevice for switches, Pc, Server, ...). Use the names \
                       to target devices in other tools.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_devices_tool(&self) -> Result<Json<DeviceList>, String> {
        list(self.packet_tracer())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_tracer::scripted::{ScriptedPacketTracer, methods};

    const NETWORK: [(&str, &str, &str); 2] = [("R1", "2911", "Router"), ("PC1", "PC-PT", "Pc")];

    fn two_devices() -> ScriptedPacketTracer {
        ScriptedPacketTracer::new(|call| {
            let steps = call.steps();
            if methods(call) == ["network", "getDeviceCount"] {
                return Ok(Value::Int(2));
            }
            let index = usize::try_from(steps[1].args[0].as_i64().unwrap()).unwrap();
            let (name, model, kind) = NETWORK[index];
            Ok(Value::qstring(match steps[2].method.as_str() {
                "getName" => name,
                "getModel" => model,
                _ => kind,
            }))
        })
    }

    #[tokio::test]
    async fn describes_each_device_in_canvas_order() {
        let listing = list(&two_devices()).await.unwrap();
        assert_eq!(
            listing.devices,
            NETWORK.map(|(name, model, kind)| Device {
                name: name.into(),
                model: model.into(),
                kind: kind.into(),
            })
        );
    }

    #[tokio::test]
    async fn empty_network_lists_nothing() {
        let packet_tracer = ScriptedPacketTracer::new(|_| Ok(Value::Int(0)));
        assert!(list(&packet_tracer).await.unwrap().devices.is_empty());
    }

    #[tokio::test]
    async fn propagates_rejections() {
        let packet_tracer =
            ScriptedPacketTracer::new(|_| Err(PtError::Rejected("Network: boom".into())));
        assert!(matches!(
            list(&packet_tracer).await,
            Err(PtError::Rejected(_))
        ));
    }
}
