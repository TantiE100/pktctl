use std::{process::Stdio, sync::Arc, time::Duration};

use pktctl::testing::{Canvas, HostAddressing};

use ptmp::{
    Call, Credentials, Event, Value,
    fake::{FAKE_PT_VERSION, FakePt, Reply},
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

const APP_ID: &str = "dev.pktctl.e2e";
const TOOLS: [&str; 14] = [
    "configure_host",
    "add_device",
    "connect",
    "disconnect",
    "list_devices",
    "list_links",
    "list_models",
    "list_ports",
    "move_device",
    "remove_device",
    "rename_device",
    "run_cli",
    "run_host_command",
    "status",
];
const SECRET: &str = "e2e-secret";

struct McpClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl McpClient {
    async fn spawn(addr: &str) -> Self {
        Self::spawn_as(addr, APP_ID, SECRET).await
    }

    async fn spawn_as(addr: &str, app_id: &str, secret: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pktctl"))
            .env("PKTCTL_ADDR", addr)
            .env("PKTCTL_APP_ID", app_id)
            .env("PKTCTL_SECRET", secret)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("pktctl binary starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut client = Self {
            _child: child,
            stdin,
            stdout,
            next_id: 1,
        };
        client.initialize().await;
        client
    }

    async fn initialize(&mut self) {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "pktctl-e2e", "version": "0" }
                }),
            )
            .await;
        assert_eq!(result["serverInfo"]["name"], "pktctl");
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }

    async fn request(&mut self, method: &str, params: Json) -> Json {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line = timeout(Duration::from_secs(10), self.stdout.next_line())
                .await
                .expect("pktctl answers in time")
                .unwrap()
                .expect("pktctl keeps stdout open");
            let message: Json = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                assert!(message.get("error").is_none(), "json-rpc error: {message}");
                return message["result"].clone();
            }
        }
    }

    async fn call_tool(&mut self, name: &str, arguments: Json) -> Json {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    async fn send(&mut self, message: Json) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }
}

async fn eventually<T>(mut check: impl FnMut() -> Option<T>) -> T {
    for _ in 0..100 {
        if let Some(value) = check() {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached within one second");
}

fn credentials() -> Credentials {
    Credentials {
        app_id: APP_ID.into(),
        secret: SECRET.into(),
    }
}

fn two_device_network(call: &Call) -> Reply {
    let methods: Vec<&str> = call
        .steps()
        .iter()
        .map(|step| step.method.as_str())
        .collect();
    let value = match methods.as_slice() {
        [
            "hardwareFactory",
            "devices",
            "getAvailableDeviceAt",
            attribute,
        ] => {
            let router = call.steps()[2].args[0] == Value::Int(0);
            match (*attribute, router) {
                ("getModel", true) => Value::qstring("2911"),
                ("getModel", false) => Value::qstring("PC-PT"),
                (_, true) => Value::Int(0),
                _ => Value::Int(8),
            }
        }
        ["network", "getDeviceCount"]
        | ["hardwareFactory", "devices", "getAvailableDeviceCount"] => Value::Int(2),
        ["network", "getLinkCount"] => Value::Int(1),
        ["network", "getDeviceAt", attribute] => {
            let router = call.steps()[1].args[0] == Value::Int(0);
            let text = match (*attribute, router) {
                ("getName", true) => "R1",
                ("getModel", true) => "2911",
                ("getClassName", true) => "Router",
                ("getName", false) => "PC1",
                ("getModel", false) => "PC-PT",
                _ => "Pc",
            };
            Value::qstring(text)
        }
        ["network", "getDevice", "enterCommand"]
            if call.steps()[1].args[0] == Value::qstring("R1") =>
        {
            Value::Pair(
                Box::new(Value::Int(0)),
                Box::new(Value::string("Cisco IOS Software, C2900")),
            )
        }
        ["network", "getDevice", "getCommandPrompt", "getObjectUuid"] => {
            Value::Uuid("{pc1-terminal}".into())
        }
        ["network", "getDevice", "getCommandPrompt", "enterCommand"] => {
            return Reply::WithEvents {
                value: Value::Void,
                events: vec![
                    terminal_event(
                        "outputWritten",
                        vec![
                            Value::string("Reply from 10.0.0.2: bytes=32 time<1ms TTL=128\n"),
                            Value::Bool(false),
                            Value::Int(0),
                        ],
                    ),
                    terminal_event(
                        "commandEnded",
                        vec![Value::string("ping 10.0.0.2"), Value::Int(0)],
                    ),
                ],
            };
        }
        ["network", "getDevice", _] => return Reply::error("Device", "IPC Cache entry: "),
        _ => return Reply::error("Network", "IPC call not found"),
    };
    Reply::Value(value)
}

fn terminal_event(name: &str, args: Vec<Value>) -> Event {
    Event {
        token: "3".into(),
        class: "TerminalLine".into(),
        object_uuid: "{pc1-terminal}".into(),
        name: name.into(),
        args,
    }
}

async fn client_with_network() -> (FakePt, McpClient) {
    let pt = FakePt::start(credentials(), two_device_network)
        .await
        .unwrap();
    let client = McpClient::spawn(&pt.addr().to_string()).await;
    (pt, client)
}

async fn client_with_canvas() -> (Arc<Canvas>, FakePt, McpClient) {
    let canvas = Arc::new(Canvas::new());
    let pt = FakePt::start(credentials(), {
        let canvas = Arc::clone(&canvas);
        move |call| match canvas.handle(call) {
            Ok(value) => Reply::Value(value),
            Err(remote) => Reply::error(remote.class, remote.message),
        }
    })
    .await
    .unwrap();
    let client = McpClient::spawn(&pt.addr().to_string()).await;
    (canvas, pt, client)
}

#[tokio::test]
async fn builds_renames_moves_and_removes_devices_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;

    let router = client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;
    assert_eq!(
        router["structuredContent"],
        json!({ "name": "R1", "model": "2911", "kind": "router", "x": 100.0, "y": 100.0 })
    );
    client
        .call_tool(
            "add_device",
            json!({ "model": "PC-PT", "x": 400, "y": 300 }),
        )
        .await;
    client
        .call_tool(
            "rename_device",
            json!({ "name": "PC0", "new_name": "PC-ADMIN" }),
        )
        .await;
    client
        .call_tool("move_device", json!({ "name": "R1", "x": 250, "y": 80 }))
        .await;

    let listing = client.call_tool("list_devices", json!({})).await;
    assert_eq!(
        listing["structuredContent"]["devices"],
        json!([
            { "name": "R1", "model": "2911", "kind": "router", "x": 250.0, "y": 80.0 },
            { "name": "PC-ADMIN", "model": "PC-PT", "kind": "pc", "x": 400.0, "y": 300.0 }
        ])
    );

    let removed = client
        .call_tool("remove_device", json!({ "name": "PC-ADMIN" }))
        .await;
    assert_eq!(
        removed["structuredContent"],
        json!({ "removed": "PC-ADMIN" })
    );
    assert_eq!(canvas.device_names(), ["R1"]);
}

#[tokio::test]
async fn cables_devices_together_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    for (model, name) in [("2911", "R1"), ("2960-24TT", "SW1"), ("PC-PT", "PC1")] {
        client
            .call_tool("add_device", json!({ "model": model, "name": name }))
            .await;
    }

    let uplink = client
        .call_tool(
            "connect",
            json!({ "device_a": "R1", "port_a": "GigabitEthernet0/0", "device_b": "SW1", "port_b": "GigabitEthernet0/1" }),
        )
        .await;
    assert_eq!(
        uplink["structuredContent"],
        json!({
            "a": { "device": "R1", "port": "GigabitEthernet0/0" },
            "b": { "device": "SW1", "port": "GigabitEthernet0/1" },
            "cable": "straight"
        })
    );
    client
        .call_tool(
            "connect",
            json!({ "device_a": "PC1", "port_a": "FastEthernet0", "device_b": "SW1", "port_b": "FastEthernet0/1" }),
        )
        .await;

    let links = client.call_tool("list_links", json!({})).await;
    assert_eq!(
        links["structuredContent"]["links"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let ports = client
        .call_tool("list_ports", json!({ "device": "PC1" }))
        .await;
    assert_eq!(
        ports["structuredContent"]["ports"][0]["connection"],
        json!({ "to": { "device": "SW1", "port": "FastEthernet0/1" }, "cable": "straight" })
    );

    let busy = client
        .call_tool(
            "connect",
            json!({ "device_a": "R1", "port_a": "GigabitEthernet0/1", "device_b": "SW1", "port_b": "FastEthernet0/1" }),
        )
        .await;
    assert_eq!(busy["isError"], true);

    client
        .call_tool(
            "disconnect",
            json!({ "device": "SW1", "port": "FastEthernet0/1" }),
        )
        .await;
    assert_eq!(canvas.links().len(), 1);
}

#[tokio::test]
async fn configures_host_addressing_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "PC-PT", "name": "PC1" }))
        .await;

    let config = client
        .call_tool(
            "configure_host",
            json!({ "device": "PC1", "ip": "192.168.10.10", "mask": "255.255.255.0", "gateway": "192.168.10.1" }),
        )
        .await;
    assert_eq!(
        config["structuredContent"],
        json!({
            "device": "PC1",
            "port": "FastEthernet0",
            "dhcp": false,
            "ip": "192.168.10.10",
            "mask": "255.255.255.0",
            "gateway": "192.168.10.1"
        })
    );
    assert_eq!(
        canvas.host_addressing("PC1", "FastEthernet0"),
        Some(HostAddressing {
            ip: "192.168.10.10".parse().unwrap(),
            mask: "255.255.255.0".parse().unwrap(),
            gateway: "192.168.10.1".parse().unwrap(),
            dns: std::net::Ipv4Addr::UNSPECIFIED,
            dhcp: false,
        })
    );

    let wrong = client
        .call_tool(
            "configure_host",
            json!({ "device": "PC1", "ip": "192.168.10.10", "mask": "255.255.255.0", "gateway": "10.0.0.1" }),
        )
        .await;
    assert_eq!(wrong["isError"], true);
}

#[tokio::test]
async fn device_mistakes_come_back_as_readable_tool_errors() {
    let (_canvas, _pt, mut client) = client_with_canvas().await;
    let unknown_model = client
        .call_tool("add_device", json!({ "model": "2960" }))
        .await;
    assert_eq!(unknown_model["isError"], true);
    assert!(
        unknown_model["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("did you mean 2960-24TT?")
    );

    let ghost = client
        .call_tool("remove_device", json!({ "name": "Ghost" }))
        .await;
    assert_eq!(ghost["content"][0]["text"], "device `Ghost` not found");
}

#[tokio::test]
async fn advertises_every_feature_tool_with_schemas() {
    let (_pt, mut client) = client_with_network().await;
    let tools = client.request("tools/list", json!({})).await;
    let mut names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    let mut expected = TOOLS;
    expected.sort_unstable();
    assert_eq!(names, expected);

    let run_cli = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "run_cli")
        .unwrap();
    assert_eq!(
        run_cli["inputSchema"]["required"],
        json!(["device", "command"])
    );
}

#[tokio::test]
async fn status_reports_live_counts() {
    let (_pt, mut client) = client_with_network().await;
    let result = client.call_tool("status", json!({})).await;
    assert_eq!(
        result["structuredContent"],
        json!({ "connected": true, "pt_version": FAKE_PT_VERSION, "devices": 2, "links": 1 })
    );
}

#[tokio::test]
async fn list_models_reads_the_hardware_catalog() {
    let (_pt, mut client) = client_with_network().await;
    let result = client
        .call_tool("list_models", json!({ "kind": "pc" }))
        .await;
    assert_eq!(
        result["structuredContent"],
        json!({ "devices": [{ "model": "PC-PT", "kind": "pc" }] })
    );
}

#[tokio::test]
async fn run_cli_returns_console_output() {
    let (pt, mut client) = client_with_network().await;
    let result = client
        .call_tool(
            "run_cli",
            json!({ "device": "R1", "command": "show version" }),
        )
        .await;
    assert_eq!(
        result["structuredContent"],
        json!({ "status": "ok", "output": "Cisco IOS Software, C2900" })
    );
    assert_eq!(pt.calls()[0].steps()[2].args[1], Value::string("enable"));
}

#[tokio::test]
async fn tool_failures_come_back_as_tool_errors_the_agent_can_read() {
    let (_pt, mut client) = client_with_network().await;
    let result = client
        .call_tool(
            "run_cli",
            json!({ "device": "R9", "command": "show version" }),
        )
        .await;
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Device")
    );
}

#[tokio::test]
async fn run_host_command_streams_console_output_until_the_command_ends() {
    let (pt, mut client) = client_with_network().await;
    let result = client
        .call_tool(
            "run_host_command",
            json!({ "device": "PC1", "command": "ping 10.0.0.2" }),
        )
        .await;
    assert_eq!(
        result["structuredContent"],
        json!({
            "finished": true,
            "status": "ok",
            "output": "Reply from 10.0.0.2: bytes=32 time<1ms TTL=128\n"
        })
    );
    let subscribed = eventually(|| {
        let subscribed: Vec<_> = pt
            .subscriptions()
            .into_iter()
            .map(|subscription| (subscription.event, subscription.enabled))
            .collect();
        (subscribed.len() == 6).then_some(subscribed)
    })
    .await;
    assert_eq!(
        subscribed,
        [
            ("outputWritten".to_owned(), true),
            ("commandEnded".to_owned(), true),
            ("moreDisplayed".to_owned(), true),
            ("outputWritten".to_owned(), false),
            ("commandEnded".to_owned(), false),
            ("moreDisplayed".to_owned(), false),
        ]
    );
}

#[tokio::test]
async fn status_explains_an_unreachable_packet_tracer() {
    let mut client = McpClient::spawn("127.0.0.1:1").await;
    let result = client.call_tool("status", json!({})).await;
    assert_eq!(result["structuredContent"]["connected"], false);
    assert!(
        result["structuredContent"]["problem"]
            .as_str()
            .unwrap()
            .contains("not reachable")
    );
}

#[tokio::test]
async fn refuses_to_start_without_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_pktctl"))
        .env_remove("PKTCTL_APP_ID")
        .env_remove("PKTCTL_SECRET")
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PKTCTL_APP_ID is not set"));
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn live_status_against_real_packet_tracer() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"));
    let addr = std::env::var("PKTCTL_ADDR").unwrap_or_else(|_| "127.0.0.1:39000".into());
    let mut client = McpClient::spawn_as(&addr, &var("PKTCTL_APP_ID"), &var("PKTCTL_SECRET")).await;
    let status = client.call_tool("status", json!({})).await;
    assert_eq!(status["structuredContent"]["connected"], true, "{status}");
}
