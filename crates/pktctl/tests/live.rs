//! Runs against a real Packet Tracer. Enable with `make e2e-live`; see docs/development.md.

mod support;

use std::time::Duration;

use serde_json::{Value as Json, json};
use support::McpClient;

const PREFIX: &str = "E2E-";
const ROUTER: &str = "E2E-R1";
const SWITCH: &str = "E2E-SW1";
const PC_A: &str = "E2E-PC1";
const PC_B: &str = "E2E-PC2";
const GATEWAY: &str = "192.168.77.1";
const PING_ATTEMPTS: u32 = 12;

async fn live_client() -> McpClient {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"));
    let addr = std::env::var("PKTCTL_ADDR").unwrap_or_else(|_| "127.0.0.1:39000".into());
    McpClient::spawn_as(&addr, &var("PKTCTL_APP_ID"), &var("PKTCTL_SECRET")).await
}

async fn ok(client: &mut McpClient, tool: &str, arguments: Json) -> Json {
    let result = client.call_tool(tool, arguments.clone()).await;
    assert_ne!(
        result["isError"], true,
        "{tool} {arguments} failed: {result}"
    );
    result["structuredContent"].clone()
}

async fn remove_leftovers(client: &mut McpClient) {
    ok(client, "simulation_mode", json!({ "on": false })).await;
    let devices = ok(client, "list_devices", json!({})).await;
    for device in devices["devices"].as_array().unwrap() {
        let name = device["name"].as_str().unwrap();
        if name.starts_with(PREFIX) {
            ok(client, "remove_device", json!({ "name": name })).await;
        }
    }
}

async fn ping(client: &mut McpClient, from: &str, target: &str) -> String {
    let mut output = String::new();
    for _ in 0..PING_ATTEMPTS {
        let result = ok(
            client,
            "run_host_command",
            json!({ "device": from, "command": format!("ping -n 2 {target}"), "timeout_secs": 30 }),
        )
        .await;
        output = result["output"].as_str().unwrap_or_default().to_owned();
        if output.contains(&format!("Reply from {target}")) {
            return output;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    panic!("{from} never reached {target}:\n{output}");
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn status_reports_a_live_connection() {
    let mut client = live_client().await;
    let status = ok(&mut client, "status", json!({})).await;
    assert_eq!(status["connected"], true, "{status}");
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn builds_a_working_lan_using_only_tools() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    build_lan(&mut client).await;
    address_lan(&mut client).await;
    verify_lan(&mut client).await;
    remove_leftovers(&mut client).await;
}

async fn build_lan(client: &mut McpClient) {
    for (model, name, x, y) in [
        ("2911", ROUTER, 300, 100),
        ("2960-24TT", SWITCH, 300, 250),
        ("PC-PT", PC_A, 200, 400),
        ("PC-PT", PC_B, 400, 400),
    ] {
        let device = ok(
            client,
            "add_device",
            json!({ "model": model, "name": name, "x": x, "y": y }),
        )
        .await;
        assert_eq!(device["name"], name);
    }

    let module = ok(
        client,
        "add_module",
        json!({ "device": ROUTER, "slot": "0/0", "module": "HWIC-2T" }),
    )
    .await;
    assert!(
        module["ports_added"]
            .as_array()
            .unwrap()
            .contains(&json!("Serial0/0/0")),
        "{module}"
    );

    for (a, port_a, b, port_b) in [
        (ROUTER, "GigabitEthernet0/0", SWITCH, "GigabitEthernet0/1"),
        (SWITCH, "FastEthernet0/1", PC_A, "FastEthernet0"),
        (SWITCH, "FastEthernet0/2", PC_B, "FastEthernet0"),
    ] {
        let link = ok(
            client,
            "connect",
            json!({ "device_a": a, "port_a": port_a, "device_b": b, "port_b": port_b }),
        )
        .await;
        assert_eq!(link["cable"], "straight", "{link}");
    }
}

async fn address_lan(client: &mut McpClient) {
    let configured = ok(
        client,
        "configure_ios",
        json!({
            "device": ROUTER,
            "commands": [
                "hostname E2E-GW",
                "interface GigabitEthernet0/0",
                format!("ip address {GATEWAY} 255.255.255.0"),
                "no shutdown"
            ],
            "save": true
        }),
    )
    .await;
    assert_eq!(configured["completed"], true, "{configured}");

    for (pc, ip) in [(PC_A, "192.168.77.10"), (PC_B, "192.168.77.11")] {
        ok(
            client,
            "configure_host",
            json!({ "device": pc, "ip": ip, "mask": "255.255.255.0", "gateway": GATEWAY }),
        )
        .await;
    }
}

async fn verify_lan(client: &mut McpClient) {
    ok(client, "fast_forward", json!({})).await;
    let ports = ok(client, "list_ports", json!({ "device": ROUTER })).await;
    let uplink = ports["ports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|port| port["name"] == "GigabitEthernet0/0")
        .unwrap();
    assert_eq!(uplink["ip"], GATEWAY);

    ping(client, PC_A, GATEWAY).await;
    ping(client, PC_A, "192.168.77.11").await;
    let from_router = ok(
        client,
        "run_cli",
        json!({ "device": ROUTER, "command": "ping 192.168.77.11", "mode": "enable" }),
    )
    .await;
    assert!(
        from_router["output"].as_str().unwrap().contains('!'),
        "{from_router}"
    );

    let links = ok(client, "list_links", json!({})).await;
    let ours = links["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|link| {
            link["a"]["device"]
                .as_str()
                .is_some_and(|device| device.starts_with(PREFIX))
        })
        .count();
    assert_eq!(ours, 3);

    let note = ok(
        client,
        "add_note",
        json!({ "x": 520, "y": 250, "text": "E2E LAN 192.168.77.0/24" }),
    )
    .await;
    let shot = client.call_tool("screenshot", json!({})).await;
    assert_eq!(shot["content"][0]["mimeType"], "image/png", "{shot}");
    ok(client, "remove_note", json!({ "id": note["id"] })).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered, including FILE"]
async fn files_round_trip_without_dialogs() {
    let mut client = live_client().await;
    let scratch = std::env::temp_dir().join(format!("pktctl-live-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let path = |name: &str| scratch.join(name).display().to_string();

    let cleared = ok(
        &mut client,
        "new_network",
        json!({ "save_current_to": path("before.pkt") }),
    )
    .await;
    assert_eq!(cleared["cleared"], true);
    let devices = ok(&mut client, "list_devices", json!({})).await;
    assert_eq!(devices["devices"], json!([]));

    ok(
        &mut client,
        "add_device",
        json!({ "model": "2911", "name": ROUTER }),
    )
    .await;
    let saved = ok(
        &mut client,
        "save_network",
        json!({ "path": path("one-router.pkt") }),
    )
    .await;
    assert!(saved["bytes"].as_u64().unwrap() > 0, "{saved}");

    ok(&mut client, "new_network", json!({})).await;
    let opened = ok(
        &mut client,
        "open_network",
        json!({ "path": path("one-router.pkt") }),
    )
    .await;
    assert!(opened["devices"].as_u64().unwrap() >= 1, "{opened}");
    let devices = ok(&mut client, "list_devices", json!({})).await;
    assert!(
        devices["devices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|device| device["name"] == ROUTER),
        "{devices}"
    );

    let missing = client
        .call_tool("open_network", json!({ "path": path("missing.pkt") }))
        .await;
    assert_eq!(missing["isError"], true);

    let restored = ok(
        &mut client,
        "open_network",
        json!({ "path": path("before.pkt") }),
    )
    .await;
    assert!(restored["devices"].as_u64().is_some(), "{restored}");
    std::fs::remove_dir_all(scratch).unwrap();
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn call_ipc_reaches_the_raw_api() {
    let mut client = live_client().await;
    let version = ok(
        &mut client,
        "call_ipc",
        json!({ "from": "appWindow", "steps": [{ "method": "getVersion" }] }),
    )
    .await;
    assert!(version["value"].as_str().unwrap().starts_with('9'));

    ok(
        &mut client,
        "add_device",
        json!({ "model": "2911", "name": ROUTER }),
    )
    .await;
    let router = ok(
        &mut client,
        "call_ipc",
        json!({ "from": "network", "steps": [{ "method": "getDevice", "args": [ROUTER] }] }),
    )
    .await;
    assert_eq!(router["value"]["class"], "Router", "{router}");

    let users = ok(
        &mut client,
        "call_ipc",
        json!({
            "from": router["value"]["uuid"],
            "steps": [{ "method": "getUserPassCount" }]
        }),
    )
    .await;
    assert!(users["value"].as_i64().is_some(), "{users}");

    let kind = ok(
        &mut client,
        "call_ipc",
        json!({ "from": "network", "steps": [
            { "method": "getDevice", "args": [ROUTER] },
            { "method": "getType" }
        ] }),
    )
    .await;
    assert_eq!(kind["value"]["name"], "ROUTER", "{kind}");
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn places_devices_in_the_physical_workspace() {
    let mut client = live_client().await;
    let saved = std::env::temp_dir().join(format!("pktctl-physical-{}.pkt", std::process::id()));
    let saved = saved.display().to_string();
    ok(
        &mut client,
        "new_network",
        json!({ "save_current_to": saved }),
    )
    .await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "2960-24TT", "name": SWITCH }),
    )
    .await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "PC-PT", "name": PC_A }),
    )
    .await;

    let city = ok(&mut client, "add_location", json!({ "kind": "city" })).await;
    let city_path = city["path"].as_str().unwrap().to_owned();
    let closet = ok(
        &mut client,
        "add_location",
        json!({ "kind": "wiring_closet", "inside": city_path, "x": 120, "y": 80 }),
    )
    .await;
    let closet_path = closet["path"].as_str().unwrap().to_owned();
    assert_eq!(
        (closet["x"].as_i64(), closet["y"].as_i64()),
        (Some(120), Some(80))
    );

    let moved = ok(
        &mut client,
        "move_to_location",
        json!({ "device": SWITCH, "into": closet_path }),
    )
    .await;
    assert_eq!(moved["now_in"], format!("{closet_path}/Rack"), "{moved}");
    let pc = ok(
        &mut client,
        "move_to_location",
        json!({ "device": PC_A, "into": city_path }),
    )
    .await;
    assert_eq!(pc["now_in"], city_path);

    let back = ok(
        &mut client,
        "move_to_location",
        json!({ "device": SWITCH, "into": "Home City/Corporate Office/Main Wiring Closet" }),
    )
    .await;
    assert_eq!(
        back["now_in"],
        "Home City/Corporate Office/Main Wiring Closet/Rack"
    );
    let renamed = ok(
        &mut client,
        "rename_location",
        json!({ "path": city_path, "name": "Cochabamba" }),
    )
    .await;
    assert_eq!(renamed["location"]["path"], "Cochabamba", "{renamed}");
    let building = ok(
        &mut client,
        "add_building",
        json!({ "inside": "Cochabamba", "name": "Alcaldía GAMC", "x": 400, "y": 150 }),
    )
    .await;
    assert_eq!(building["location"]["path"], "Cochabamba/Alcaldía GAMC");
    assert_eq!(building["location"]["kind"], "building");
    let devices = ok(&mut client, "list_devices", json!({})).await;
    let power_units = devices["devices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|device| device["model"] == "Power Distribution Device")
        .count();
    assert!(
        power_units <= 2,
        "reopening must not pile up power units: {devices}"
    );
    let shown = ok(&mut client, "show_workspace", json!({ "view": "physical" })).await;
    assert_eq!(shown["physical"], true);
    ok(&mut client, "show_workspace", json!({ "view": "logical" })).await;
    ok(&mut client, "open_network", json!({ "path": saved })).await;
    std::fs::remove_file(saved).unwrap();
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn follows_a_ping_in_simulation_mode() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "2960-24TT", "name": SWITCH }),
    )
    .await;
    for (pc, port, ip) in [
        (PC_A, "FastEthernet0/1", "10.77.0.1"),
        (PC_B, "FastEthernet0/2", "10.77.0.2"),
    ] {
        ok(
            &mut client,
            "add_device",
            json!({ "model": "PC-PT", "name": pc }),
        )
        .await;
        ok(
            &mut client,
            "connect",
            json!({ "device_a": SWITCH, "port_a": port, "device_b": pc, "port_b": "FastEthernet0" }),
        )
        .await;
        ok(
            &mut client,
            "configure_host",
            json!({ "device": pc, "ip": ip, "mask": "255.255.255.0" }),
        )
        .await;
    }
    ok(&mut client, "fast_forward", json!({})).await;
    ping(&mut client, PC_A, "10.77.0.2").await;
    ok(&mut client, "simulation_mode", json!({ "on": true })).await;
    ok(&mut client, "simulation_step", json!({ "action": "reset" })).await;
    ok(
        &mut client,
        "add_pdu",
        json!({ "source": PC_A, "destination": PC_B }),
    )
    .await;
    let mut reply = Json::Null;
    for _ in 0..30 {
        ok(&mut client, "simulation_step", json!({})).await;
        let events = ok(
            &mut client,
            "list_simulation_events",
            json!({ "protocols": ["ICMP"], "device": PC_A, "include_decisions": true }),
        )
        .await;
        reply = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["from"] == SWITCH)
            .cloned()
            .unwrap_or(Json::Null);
        if !reply.is_null() {
            break;
        }
    }
    assert!(!reply.is_null(), "the echo reply never came back to {PC_A}");
    assert!(
        reply["decisions"]
            .as_array()
            .is_some_and(|decisions| !decisions.is_empty())
    );
    ok(&mut client, "simulation_mode", json!({ "on": false })).await;
    remove_leftovers(&mut client).await;
}
