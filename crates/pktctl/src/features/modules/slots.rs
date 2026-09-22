use futures::future::{BoxFuture, FutureExt, try_join_all};
use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    features::{devices::describe, paths::device},
    packet_tracer::{PacketTracer, PtError, expect_integer, expect_text, kinds::module_kind},
};

const FIXED_KINDS: &[&str] = &["non_removable_module", "non_removable_interface_card"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Slot {
    pub path: String,
    pub accepts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub accepts_code: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SlotList {
    pub device: String,
    pub slots: Vec<Slot>,
    pub supported_modules: Vec<String>,
}

pub async fn list_slots<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
) -> Result<SlotList, PtError> {
    let device_name = device_name.trim();
    describe(packet_tracer, device_name).await?;
    let root = device(device_name).method("getRootModule", []);
    let (slots, supported) = tokio::try_join!(
        walk(packet_tracer, root, None),
        supported_modules(packet_tracer, device_name),
    )?;
    Ok(SlotList {
        device: device_name.to_owned(),
        slots,
        supported_modules: supported,
    })
}

pub(super) async fn supported_modules<P: PacketTracer>(
    packet_tracer: &P,
    device_name: &str,
) -> Result<Vec<String>, PtError> {
    let reply = packet_tracer
        .call(device(device_name).method("getSupportedModule", []))
        .await?;
    let entries = reply
        .into_items()
        .ok_or_else(|| PtError::UnexpectedReply("supported modules should be a list".into()))?;
    entries
        .iter()
        .map(|entry| {
            let text = expect_text(entry, "supported module")?;
            Ok(text.split(':').next().unwrap_or_default().to_owned())
        })
        .collect()
}

fn walk<P: PacketTracer>(
    packet_tracer: &P,
    module: Call,
    path: Option<String>,
) -> BoxFuture<'_, Result<Vec<Slot>, PtError>> {
    async move {
        let count = packet_tracer
            .call(module.clone().method("getSlotCount", []))
            .await?;
        let count = i32::try_from(expect_integer(&count, "slot count")?)
            .map_err(|_| PtError::UnexpectedReply("slot count out of range".into()))?;

        let children = try_join_all((0..count).map(|index| {
            let module = module.clone();
            let child_path = path
                .as_ref()
                .map_or_else(|| index.to_string(), |parent| format!("{parent}/{index}"));
            async move {
                let accepts = packet_tracer
                    .call(module.clone().method("getSlotTypeAt", [Value::Int(index)]))
                    .await?;
                let accepts_code = expect_integer(&accepts, "slot type")?;
                let child = module.method("getModuleAt", [Value::Int(index)]);
                let installed = match packet_tracer
                    .call(
                        child
                            .clone()
                            .method("getDescriptor", [])
                            .method("getModel", []),
                    )
                    .await
                {
                    Ok(model) => Some(expect_text(&model, "module model")?),
                    Err(PtError::NotFound(_)) => None,
                    Err(other) => return Err(other),
                };
                let mut found = Vec::new();
                let kind = module_kind(accepts_code);
                if !FIXED_KINDS.contains(&kind.as_str()) {
                    found.push(Slot {
                        path: child_path.clone(),
                        accepts: kind,
                        installed: installed.clone(),
                        accepts_code,
                    });
                }
                if installed.is_some() {
                    found.extend(walk(packet_tracer, child, Some(child_path)).await?);
                }
                Ok::<_, PtError>(found)
            }
        }))
        .await?;
        Ok(children.into_iter().flatten().collect())
    }
    .boxed()
}
