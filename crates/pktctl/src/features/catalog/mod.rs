use futures::future::try_join_all;
use ptmp::{Call, Value};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    packet_tracer::{
        PacketTracer, PtError, expect_integer, expect_text,
        kinds::{device_kind, device_kind_names, module_kind},
    },
    server::PktctlServer,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Model {
    pub model: String,
    pub kind: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub type_code: i32,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CatalogRequest {
    /// Only device models of this kind, for example `router`, `switch`, `pc`, `server`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Also list the modules (HWIC, NIM, NM cards) that can be installed in slots.
    #[serde(default)]
    pub include_modules: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Catalog {
    pub devices: Vec<Model>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Model>,
}

#[derive(Debug, Clone, Copy)]
enum Factory {
    Devices,
    Modules,
}

impl Factory {
    fn root(self) -> Call {
        let (factory, _) = self.names();
        Call::root("hardwareFactory").method(factory, [])
    }

    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Devices => ("devices", "Device"),
            Self::Modules => ("modules", "Module"),
        }
    }

    fn kind(self, code: i64) -> String {
        match self {
            Self::Devices => device_kind(code),
            Self::Modules => module_kind(code),
        }
    }
}

pub async fn device_models<P: PacketTracer>(packet_tracer: &P) -> Result<Vec<Model>, PtError> {
    models(packet_tracer, Factory::Devices).await
}

pub async fn module_models<P: PacketTracer>(packet_tracer: &P) -> Result<Vec<Model>, PtError> {
    models(packet_tracer, Factory::Modules).await
}

pub async fn find_device_model<P: PacketTracer>(
    packet_tracer: &P,
    model: &str,
) -> Result<Model, PtError> {
    find(&device_models(packet_tracer).await?, model, "device")
}

pub async fn find_module_model<P: PacketTracer>(
    packet_tracer: &P,
    model: &str,
) -> Result<Model, PtError> {
    find(&module_models(packet_tracer).await?, model, "module")
}

pub async fn list<P: PacketTracer>(
    packet_tracer: &P,
    request: &CatalogRequest,
) -> Result<Catalog, PtError> {
    let kind = request
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty());
    if let Some(kind) = kind
        && !device_kind_names().any(|name| name == kind)
    {
        let known: Vec<_> = device_kind_names().collect();
        return Err(PtError::InvalidInput(format!(
            "unknown kind `{kind}`; use one of: {}",
            known.join(", ")
        )));
    }

    let mut devices = device_models(packet_tracer).await?;
    if let Some(kind) = kind {
        devices.retain(|model| model.kind == kind);
    }
    let modules = if request.include_modules {
        module_models(packet_tracer).await?
    } else {
        Vec::new()
    };
    Ok(Catalog { devices, modules })
}

async fn models<P: PacketTracer>(
    packet_tracer: &P,
    factory: Factory,
) -> Result<Vec<Model>, PtError> {
    let (_, noun) = factory.names();
    let count = packet_tracer
        .call(
            factory
                .root()
                .method(format!("getAvailable{noun}Count"), []),
        )
        .await?;
    let count = i32::try_from(expect_integer(&count, "catalog size")?)
        .map_err(|_| PtError::UnexpectedReply("catalog size out of range".into()))?;

    let entries = try_join_all((0..count).map(|index| async move {
        let at = || {
            factory
                .root()
                .method(format!("getAvailable{noun}At"), [Value::Int(index)])
        };
        let (model, code) = tokio::try_join!(
            packet_tracer.call(at().method("getModel", [])),
            packet_tracer.call(at().method("getType", [])),
        )?;
        let code = expect_integer(&code, "model type")?;
        Ok::<_, PtError>(Model {
            model: expect_text(&model, "model name")?,
            kind: factory.kind(code),
            type_code: i32::try_from(code)
                .map_err(|_| PtError::UnexpectedReply(format!("type code {code} out of range")))?,
        })
    }))
    .await?;

    let mut unique = Vec::with_capacity(entries.len());
    for entry in entries {
        if !unique.contains(&entry) {
            unique.push(entry);
        }
    }
    Ok(unique)
}

fn find(models: &[Model], wanted: &str, noun: &str) -> Result<Model, PtError> {
    let wanted = wanted.trim();
    if let Some(exact) = models.iter().find(|model| model.model == wanted) {
        return Ok(exact.clone());
    }
    let mut folded = models
        .iter()
        .filter(|model| model.model.eq_ignore_ascii_case(wanted));
    if let (Some(only), None) = (folded.next(), folded.next()) {
        return Ok(only.clone());
    }

    let needle = wanted.to_ascii_lowercase();
    let similar: Vec<_> = models
        .iter()
        .filter(|model| model.model.to_ascii_lowercase().contains(&needle))
        .map(|model| model.model.as_str())
        .take(8)
        .collect();
    let hint = if similar.is_empty() {
        "call list_models to see what is available".to_owned()
    } else {
        format!("did you mean {}?", similar.join(", "))
    };
    Err(PtError::InvalidInput(format!(
        "unknown {noun} model `{wanted}`; {hint}"
    )))
}

#[tool_router(router = catalog_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "list_models",
        description = "List the device models this Packet Tracer can create, straight from its \
                       hardware catalog, optionally filtered by kind (router, switch, pc, \
                       server, access_point, ...). Set include_modules to also list slot \
                       modules such as HWIC-2T or NIM-2T.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_models_tool(
        &self,
        Parameters(request): Parameters<CatalogRequest>,
    ) -> Result<Json<Catalog>, String> {
        list(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_tracer::scripted::{ScriptedPacketTracer, methods};

    const DEVICES: [(&str, i32); 4] = [("1841", 0), ("1841", 0), ("2960-24TT", 1), ("PC-PT", 8)];
    const MODULES: [(&str, i32); 2] = [("HWIC-2T", 2), ("NIM-2T", 1)];

    fn hardware() -> ScriptedPacketTracer {
        ScriptedPacketTracer::new(|call| {
            let steps = call.steps();
            let table: &[(&str, i32)] = match steps[1].method.as_str() {
                "devices" => &DEVICES,
                _ => &MODULES,
            };
            Ok(match methods(call).as_slice() {
                [_, _, count] if count.ends_with("Count") => {
                    Value::Int(i32::try_from(table.len()).unwrap())
                }
                [_, _, _, "getModel"] => {
                    let index = usize::try_from(steps[2].args[0].as_i64().unwrap()).unwrap();
                    Value::qstring(table[index].0)
                }
                [_, _, _, "getType"] => {
                    let index = usize::try_from(steps[2].args[0].as_i64().unwrap()).unwrap();
                    Value::Int(table[index].1)
                }
                other => panic!("unexpected call {other:?}"),
            })
        })
    }

    fn request(kind: Option<&str>, include_modules: bool) -> CatalogRequest {
        CatalogRequest {
            kind: kind.map(str::to_owned),
            include_modules,
        }
    }

    #[tokio::test]
    async fn lists_unique_device_models_with_readable_kinds() {
        let catalog = list(&hardware(), &request(None, false)).await.unwrap();
        let listed: Vec<_> = catalog
            .devices
            .iter()
            .map(|model| (model.model.as_str(), model.kind.as_str()))
            .collect();
        assert_eq!(
            listed,
            [("1841", "router"), ("2960-24TT", "switch"), ("PC-PT", "pc")]
        );
        assert!(catalog.modules.is_empty());
    }

    #[tokio::test]
    async fn filters_by_kind_and_adds_modules_on_request() {
        let catalog = list(&hardware(), &request(Some("switch"), true))
            .await
            .unwrap();
        assert_eq!(catalog.devices.len(), 1);
        assert_eq!(catalog.modules[0].kind, "interface_card");
        assert_eq!(catalog.modules[1].kind, "network_module");
    }

    #[tokio::test]
    async fn rejects_unknown_kinds_with_the_valid_list() {
        let error = list(&hardware(), &request(Some("toaster"), false))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("router"));
    }

    #[tokio::test]
    async fn finds_models_exactly_or_case_insensitively() {
        let pc = find_device_model(&hardware(), "pc-pt").await.unwrap();
        assert_eq!((pc.model.as_str(), pc.type_code), ("PC-PT", 8));
        let card = find_module_model(&hardware(), "HWIC-2T").await.unwrap();
        assert_eq!(card.type_code, 2);
    }

    #[tokio::test]
    async fn suggests_similar_models_when_the_name_is_wrong() {
        let error = find_device_model(&hardware(), "2960").await.unwrap_err();
        assert!(error.to_string().contains("did you mean 2960-24TT?"));
        let error = find_device_model(&hardware(), "zzz").await.unwrap_err();
        assert!(error.to_string().contains("list_models"));
    }
}
