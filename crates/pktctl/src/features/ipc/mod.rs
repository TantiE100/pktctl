mod call;
mod describe;

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};

pub(crate) use call::plain;
pub use call::{IpcCallRequest, IpcCallResult, IpcStep, call_ipc};
pub use describe::{ClassInfo, DescribeRequest, Description, EnumInfo, MethodInfo, describe};

use crate::{packet_tracer::PacketTracer, server::PktctlServer};

#[tool_router(router = ipc_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "describe_ipc",
        description = "Explore Packet Tracer's complete IPC API (every class, method, argument \
                       and enum of the official framework). Give `search` to find methods by \
                       keyword, `class` to list everything an object offers, `enum` to read \
                       accepted values, or nothing for the list of roots. Use it before \
                       call_ipc for anything the dedicated tools do not cover.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn describe_ipc_tool(
        &self,
        Parameters(request): Parameters<DescribeRequest>,
    ) -> Result<Json<Description>, String> {
        describe(&request)
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "call_ipc",
        description = "Call any Packet Tracer IPC method: start at a root or an object uuid and \
                       chain `steps`, like `network` -> getDevice(\"R1\") -> getPower(). Every \
                       step is checked against the official API before anything is sent, with \
                       exact argument types; objects come back as a class and uuid you can \
                       start the next call from. Prefer the dedicated tools when one fits; this \
                       reaches everything else, including changes, so read describe_ipc first.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn call_ipc_tool(
        &self,
        Parameters(request): Parameters<IpcCallRequest>,
    ) -> Result<Json<IpcCallResult>, String> {
        call_ipc(self.packet_tracer(), &request)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ptmp::Value;
    use serde_json::json;

    use super::*;
    use crate::{
        features::devices::{AddDeviceRequest, add},
        packet_tracer::{
            PtError,
            scripted::{ScriptedPacketTracer, methods},
        },
        testing::Canvas,
    };

    fn step(method: &str, args: serde_json::Value) -> IpcStep {
        let serde_json::Value::Array(args) = args else {
            panic!("step arguments must be an array");
        };
        IpcStep {
            method: method.into(),
            args,
        }
    }

    fn request(from: &str, steps: Vec<IpcStep>) -> IpcCallRequest {
        IpcCallRequest {
            from: from.into(),
            steps,
        }
    }

    async fn lab() -> (Arc<Canvas>, ScriptedPacketTracer) {
        let canvas = Arc::new(Canvas::new());
        let packet_tracer = ScriptedPacketTracer::on_canvas(Arc::clone(&canvas));
        for name in ["R1", "R2"] {
            let request = AddDeviceRequest {
                model: "2911".into(),
                name: Some(name.into()),
                ..AddDeviceRequest::default()
            };
            add(&packet_tracer, &request).await.unwrap();
        }
        (canvas, packet_tracer)
    }

    #[tokio::test]
    async fn chains_calls_with_the_exact_wire_types() {
        let (_canvas, packet_tracer) = lab().await;
        let result = call_ipc(
            &packet_tracer,
            &request(
                "network",
                vec![
                    step("getDevice", json!(["R1"])),
                    step("getPower", json!([])),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.call, r#"network.getDevice("R1").getPower()"#);
        assert_eq!(result.returns, "bool");
        assert_eq!(result.value, json!(true));
    }

    #[tokio::test]
    async fn accepts_enums_by_name_and_reports_them_by_name() {
        let (canvas, packet_tracer) = lab().await;
        let created = call_ipc(
            &packet_tracer,
            &request(
                "appWindow",
                vec![
                    step("getActiveWorkspace", json!([])),
                    step("getLogicalWorkspace", json!([])),
                    step(
                        "createLink",
                        json!([
                            "R1",
                            "GigabitEthernet0/0",
                            "R2",
                            "GigabitEthernet0/0",
                            "ethernet_cross"
                        ]),
                    ),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(created.value, json!(true));
        assert_eq!(canvas.links()[0].cable, 8101);

        let kind = call_ipc(
            &packet_tracer,
            &request(
                "network",
                vec![
                    step("getLinkAt", json!([0])),
                    step("getConnectionType", json!([])),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(kind.returns, "ConnectType");
        assert_eq!(
            kind.value,
            json!({ "name": "ETHERNET_CROSS", "value": 8101 })
        );
    }

    #[tokio::test]
    async fn explains_mistakes_before_sending_anything() {
        let packet_tracer = ScriptedPacketTracer::new(|call| match methods(call).as_slice() {
            [.., "getClassName"] => Ok(Value::qstring("Router")),
            other => panic!("nothing else may reach Packet Tracer: {other:?}"),
        });
        let mistakes = [
            (request("nowhere", vec![]), "must be a root"),
            (
                request("network", vec![step("getDevise", json!(["R1"]))]),
                "no method `getDevise`",
            ),
            (
                request("network", vec![step("getDevice", json!([]))]),
                "different number of arguments",
            ),
            (
                request("network", vec![step("getDeviceAt", json!(["first"]))]),
                "must be an integer",
            ),
            (
                request(
                    "network",
                    vec![
                        step("getDeviceCount", json!([])),
                        step("getName", json!([])),
                    ],
                ),
                "nothing can be called on it",
            ),
        ];
        for (bad, expected) in mistakes {
            let error = call_ipc(&packet_tracer, &bad).await.unwrap_err();
            assert!(matches!(error, PtError::InvalidInput(_)), "{error}");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[tokio::test]
    async fn finds_methods_of_the_real_class_behind_a_declared_one() {
        let packet_tracer = ScriptedPacketTracer::new(|call| match methods(call).as_slice() {
            ["network", "getDevice", "getClassName"] => Ok(Value::qstring("Router")),
            ["network", "getDevice", "getUserPassCount"] => Ok(Value::Int(2)),
            ["network", "getDevice", "getProcess", "getClassName"] => {
                Ok(Value::string("ArpProcess"))
            }
            ["network", "getDevice", "getProcess", "getArpTable"] => Ok(Value::Int(0)),
            other => panic!("unexpected call {other:?}"),
        });
        let result = call_ipc(
            &packet_tracer,
            &request(
                "network",
                vec![
                    step("getDevice", json!(["R1"])),
                    step("getUserPassCount", json!([])),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.value, json!(2));

        let arp = call_ipc(
            &packet_tracer,
            &request(
                "network",
                vec![
                    step("getDevice", json!(["PC1"])),
                    step("getProcess", json!(["ArpProcess"])),
                    step("getArpTable", json!([])),
                ],
            ),
        )
        .await;
        assert!(
            arp.is_ok(),
            "wire class names differ in case from the Java ones: {arp:?}"
        );
    }

    #[tokio::test]
    async fn returns_objects_as_references_and_resumes_from_them() {
        let packet_tracer = ScriptedPacketTracer::new(|call| {
            let steps = call.steps();
            match methods(call).as_slice() {
                ["network", "getDevice", "getObjectUuid"] => Ok(Value::Uuid("{r1}".into())),
                ["network", "getDevice", "getClassName"] | ["getObjectByUuid", "getClassName"] => {
                    Ok(Value::qstring("Router"))
                }
                ["getObjectByUuid", "getName"] => {
                    assert_eq!(steps[0].args, [Value::string("{r1}")]);
                    Ok(Value::qstring("R1"))
                }
                other => panic!("unexpected call {other:?}"),
            }
        });
        let object = call_ipc(
            &packet_tracer,
            &request("network", vec![step("getDevice", json!(["R1"]))]),
        )
        .await
        .unwrap();
        assert_eq!(object.returns, "Device");
        assert_eq!(object.value, json!({ "class": "Router", "uuid": "{r1}" }));

        let name = call_ipc(
            &packet_tracer,
            &request("{r1}", vec![step("getName", json!([]))]),
        )
        .await
        .unwrap();
        assert_eq!(name.value, json!("R1"));
    }

    #[tokio::test]
    async fn value_objects_come_back_with_field_names() {
        let packet_tracer = ScriptedPacketTracer::new(|_| {
            Ok(Value::Data {
                class: "FlowChartNode".into(),
                fields: vec![
                    Value::string("CPingProcess_next_ping"),
                    Value::qstring("The Ping process starts the next ping request."),
                    Value::Bool(false),
                    Value::Int(3),
                ],
            })
        });
        let node = call_ipc(
            &packet_tracer,
            &request(
                "simulation",
                vec![
                    step("getFrameInstanceAt", json!([0])),
                    step("getFlowChartNodeAt", json!([0])),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            node.value,
            json!({
                "class": "FlowChartNode",
                "strID": "CPingProcess_next_ping",
                "description": "The Ping process starts the next ping request.",
                "isOSIIn": false,
                "OSILayerNumber": 3
            })
        );
    }

    #[tokio::test]
    async fn byte_lists_come_back_encoded() {
        let packet_tracer = ScriptedPacketTracer::new(|_| Ok(Value::Bytes(vec![137, 80, 78, 71])));
        let image = call_ipc(
            &packet_tracer,
            &request(
                "appWindow",
                vec![
                    step("getActiveWorkspace", json!([])),
                    step("getLogicalWorkspace", json!([])),
                    step("getWorkspaceImage", json!(["PNG"])),
                ],
            ),
        )
        .await
        .unwrap();
        assert_eq!(image.value, json!({ "bytes": 4, "base64": "iVBORw==" }));
    }
}
