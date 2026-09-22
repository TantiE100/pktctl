mod support;

use std::{sync::Arc, time::Duration};

use pktctl::testing::{Canvas, HostAddressing};

use ptmp::{
    Call, Credentials, Event, Value,
    fake::{FAKE_PT_VERSION, FakePt, Reply},
};
use serde_json::json;
use support::McpClient;
use tokio::process::Command;

const APP_ID: &str = "dev.pktctl.e2e";
const TOOLS: [&str; 41] = [
    "fast_forward",
    "power_cycle_all",
    "set_power",
    "add_pdu",
    "list_simulation_events",
    "simulation_mode",
    "simulation_step",
    "add_building",
    "rename_location",
    "add_location",
    "list_locations",
    "move_to_location",
    "show_workspace",
    "call_ipc",
    "describe_ipc",
    "setup_exapp",
    "add_note",
    "list_notes",
    "new_network",
    "open_network",
    "remove_note",
    "save_network",
    "screenshot",
    "add_module",
    "list_slots",
    "remove_module",
    "configure_ios",
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

async fn spawn(addr: &str) -> McpClient {
    McpClient::spawn_as(addr, APP_ID, SECRET).await
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
    let client = spawn(&pt.addr().to_string()).await;
    (pt, client)
}

async fn client_with_canvas() -> (Arc<Canvas>, FakePt, McpClient) {
    let canvas = Arc::new(Canvas::new());
    let pt = FakePt::start(credentials(), {
        let canvas = Arc::clone(&canvas);
        move |call| match canvas.handle(call) {
            Ok(value) => Reply::WithEvents {
                value,
                events: canvas.take_events(),
            },
            Err(remote) => Reply::error(remote.class, remote.message),
        }
    })
    .await
    .unwrap();
    let client = spawn(&pt.addr().to_string()).await;
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
async fn applies_ios_configuration_blocks_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;

    let result = client
        .call_tool(
            "configure_ios",
            json!({
                "device": "R1",
                "commands": ["conf t", "interface GigabitEthernet0/0", "ip address 10.0.0.1 255.255.255.0", "no shutdown"],
                "save": true
            }),
        )
        .await;
    assert_eq!(result["structuredContent"]["completed"], true);
    assert_eq!(result["structuredContent"]["saved"], true);
    assert_eq!(result["structuredContent"]["applied"], 3);

    let typed: Vec<String> = canvas
        .cli_history("R1")
        .into_iter()
        .map(|(_, command)| command)
        .collect();
    assert_eq!(
        typed,
        [
            "interface GigabitEthernet0/0",
            "ip address 10.0.0.1 255.255.255.0",
            "no shutdown",
            "end",
            "write memory"
        ]
    );
}

#[tokio::test]
async fn installs_modules_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;

    let slots = client
        .call_tool("list_slots", json!({ "device": "R1" }))
        .await;
    assert_eq!(
        slots["structuredContent"]["supported_modules"],
        json!(["HWIC-2T"])
    );

    let installed = client
        .call_tool(
            "add_module",
            json!({ "device": "R1", "slot": "0/1", "module": "HWIC-2T" }),
        )
        .await;
    assert_eq!(
        installed["structuredContent"]["ports_added"],
        json!(["Serial0/1/0", "Serial0/1/1"])
    );
    assert_eq!(canvas.installed_cards("R1")[1].as_deref(), Some("HWIC-2T"));

    let removed = client
        .call_tool("remove_module", json!({ "device": "R1", "slot": "0/1" }))
        .await;
    assert_eq!(
        removed["structuredContent"]["ports_removed"],
        json!(["Serial0/1/0", "Serial0/1/1"])
    );
}

#[tokio::test]
async fn saves_reopens_and_captures_the_workspace_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;
    let saved = client
        .call_tool("save_network", json!({ "path": "/labs/e2e.pkt" }))
        .await;
    assert_eq!(saved["structuredContent"]["path"], "/labs/e2e.pkt");

    client.call_tool("new_network", json!({})).await;
    assert!(canvas.device_names().is_empty());

    let opened = client
        .call_tool("open_network", json!({ "path": "/labs/e2e.pkt" }))
        .await;
    assert_eq!(opened["structuredContent"]["devices"], 1);

    let shot = client.call_tool("screenshot", json!({})).await;
    assert_eq!(shot["content"][0]["type"], "image");
    assert_eq!(shot["content"][0]["mimeType"], "image/png");

    let note = client
        .call_tool("add_note", json!({ "x": 50, "y": 20, "text": "LAN A" }))
        .await;
    assert_eq!(note["structuredContent"]["text"], "LAN A");
    let notes = client.call_tool("list_notes", json!({})).await;
    assert_eq!(
        notes["structuredContent"]["notes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[cfg(unix)]
#[tokio::test]
async fn setup_exapp_builds_the_registration_file_before_packet_tracer_accepts_us() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("pktctl-e2e-setup-{}", std::process::id()));
    let extensions = root.join("pt/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    let meta = extensions.join("meta");
    std::fs::write(&meta, "#!/bin/sh\ncp \"$2\" \"$1\"\n").unwrap();
    std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = root.join("out");
    let (pt_home, out) = (
        root.join("pt").display().to_string(),
        output.display().to_string(),
    );

    let mut client = McpClient::spawn_with(
        "127.0.0.1:1",
        APP_ID,
        SECRET,
        &[("PKTCTL_PT_HOME", &pt_home), ("PKTCTL_SETUP_DIR", &out)],
    )
    .await;
    let result = client.call_tool("setup_exapp", json!({})).await;
    let pta = output.join("pktctl.pta");
    assert_eq!(
        result["structuredContent"]["pta"],
        pta.display().to_string()
    );
    let contents = std::fs::read_to_string(&pta).unwrap();
    assert!(contents.contains(&format!("<ID>{APP_ID}</ID>")));
    assert!(contents.contains(&format!("<KEY>{SECRET}</KEY>")));
    assert!(!output.join("pktctl-exapp.xml").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn call_ipc_reaches_any_method_end_to_end() {
    let (_canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;
    let power = client
        .call_tool(
            "call_ipc",
            json!({
                "from": "network",
                "steps": [
                    { "method": "getDevice", "args": ["R1"] },
                    { "method": "getPower" }
                ]
            }),
        )
        .await;
    assert_eq!(
        power["structuredContent"],
        json!({ "call": "network.getDevice(\"R1\").getPower()", "returns": "bool", "value": true })
    );

    let typo = client
        .call_tool(
            "call_ipc",
            json!({ "from": "network", "steps": [{ "method": "getDevise", "args": ["R1"] }] }),
        )
        .await;
    assert_eq!(typo["isError"], true);
    assert!(
        typo["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("did you mean getDevice")
    );

    let described = client
        .call_tool("describe_ipc", json!({ "class": "Network" }))
        .await;
    assert!(
        described["structuredContent"]["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method["signature"] == "getDevice(deviceName: string) -> Device")
    );
}

#[tokio::test]
async fn places_devices_in_the_physical_workspace_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2960-24TT", "name": "S1" }))
        .await;
    let closet = client
        .call_tool(
            "add_location",
            json!({ "kind": "wiring_closet", "inside": "Home City/Corporate Office" }),
        )
        .await;
    assert_eq!(
        closet["structuredContent"]["path"],
        "Home City/Corporate Office/Wiring Closet"
    );
    let moved = client
        .call_tool(
            "move_to_location",
            json!({ "device": "S1", "into": "Home City/Corporate Office/Wiring Closet" }),
        )
        .await;
    assert_eq!(
        moved["structuredContent"],
        json!({ "moved": "S1", "now_in": "Home City/Corporate Office/Wiring Closet/Rack" })
    );
    assert_eq!(canvas.physical_parent("S1").as_deref(), Some("Rack"));
}

#[tokio::test]
async fn simulates_a_ping_end_to_end() {
    let (_canvas, _pt, mut client) = client_with_canvas().await;
    for name in ["PC1", "PC2"] {
        client
            .call_tool("add_device", json!({ "model": "PC-PT", "name": name }))
            .await;
    }
    client
        .call_tool("simulation_mode", json!({ "on": true }))
        .await;
    client
        .call_tool("add_pdu", json!({ "source": "PC1", "destination": "PC2" }))
        .await;
    let stepped = client.call_tool("simulation_step", json!({})).await;
    assert_eq!(stepped["structuredContent"]["events"], 2);
    let events = client
        .call_tool("list_simulation_events", json!({ "protocols": ["ICMP"] }))
        .await;
    assert_eq!(events["structuredContent"]["events"][1]["device"], "PC2");
    assert_eq!(
        events["structuredContent"]["events"][1]["status"],
        json!(["accepted"])
    );
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
async fn run_cli_waits_for_console_output_end_to_end() {
    let (canvas, _pt, mut client) = client_with_canvas().await;
    client
        .call_tool("add_device", json!({ "model": "2911", "name": "R1" }))
        .await;
    let result = client
        .call_tool(
            "run_cli",
            json!({ "device": "R1", "command": "ping 10.0.0.2" }),
        )
        .await;
    let run = &result["structuredContent"];
    assert_eq!(run["finished"], true, "{result}");
    assert_eq!(run["status"], "ok");
    assert!(
        run["output"]
            .as_str()
            .unwrap()
            .contains("!!!!!\nSuccess rate is 100 percent"),
        "{result}"
    );
    assert_eq!(canvas.console_prompt("R1").as_deref(), Some("Router#"));
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
    assert_eq!(result["content"][0]["text"], "device `R9` not found");
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
    let mut client = spawn("127.0.0.1:1").await;
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
