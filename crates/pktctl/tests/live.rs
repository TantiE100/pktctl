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
    let city_path = city["location"]["path"].as_str().unwrap().to_owned();
    let closet = ok(
        &mut client,
        "add_location",
        json!({ "kind": "wiring_closet", "inside": city_path, "x": 120, "y": 80 }),
    )
    .await;
    let closet_path = closet["location"]["path"].as_str().unwrap().to_owned();
    assert_eq!(
        (
            closet["location"]["x"].as_i64(),
            closet["location"]["y"].as_i64()
        ),
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
        "add_location",
        json!({ "kind": "building", "inside": "Cochabamba", "name": "Alcaldía GAMC",
                "x": 400, "y": 150 }),
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

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered, including FILE"]
async fn joins_a_wpa2_network() {
    let mut client = live_client().await;
    let saved = std::env::temp_dir().join(format!("pktctl-wifi-{}.pkt", std::process::id()));
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
        json!({ "model": "AccessPoint-PT", "name": "E2E-AP" }),
    )
    .await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "Laptop-PT", "name": "E2E-LT" }),
    )
    .await;
    ok(
        &mut client,
        "move_to_location",
        json!({ "device": "E2E-AP", "into": "Home City/Corporate Office" }),
    )
    .await;
    ok(
        &mut client,
        "remove_module",
        json!({ "device": "E2E-LT", "slot": "0" }),
    )
    .await;
    ok(
        &mut client,
        "add_module",
        json!({ "device": "E2E-LT", "slot": "0", "module": "PT-LAPTOP-NM-1W" }),
    )
    .await;
    ok(
        &mut client,
        "configure_access_point",
        json!({ "device": "E2E-AP", "ssid": "GAMC", "security": "wpa2_psk", "key": "clave1234" }),
    )
    .await;
    let join = |key: &str| {
        json!({ "device": "E2E-LT", "ssid": "GAMC", "security": "wpa2_psk", "key": key,
                "ip": "192.168.50.20", "mask": "255.255.255.0" })
    };
    let refused = ok(&mut client, "connect_wireless", join("incorrecta")).await;
    assert_eq!(refused["associated"], false, "{refused}");
    let joined = ok(&mut client, "connect_wireless", join("clave1234")).await;
    assert_eq!(joined["associated"], true, "{joined}");
    assert_eq!(joined["access_point"], "E2E-AP");
    assert_eq!(joined["ip"], "192.168.50.20");
    let status = ok(
        &mut client,
        "wireless_status",
        json!({ "device": "E2E-LT" }),
    )
    .await;
    assert_eq!(status["access_point"], "E2E-AP");
    ok(&mut client, "open_network", json!({ "path": saved })).await;
    std::fs::remove_file(saved).unwrap();
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn serves_dhcp_and_dns_to_a_pc() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "Server-PT", "name": "E2E-SRV" }),
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
    for (device, port) in [("E2E-SRV", "FastEthernet0/1"), (PC_A, "FastEthernet0/2")] {
        ok(
            &mut client,
            "connect",
            json!({ "device_a": SWITCH, "port_a": port, "device_b": device, "port_b": "FastEthernet0" }),
        )
        .await;
    }
    ok(
        &mut client,
        "configure_host",
        json!({ "device": "E2E-SRV", "ip": "192.168.60.5", "mask": "255.255.255.0", "gateway": "192.168.60.1" }),
    )
    .await;
    let dhcp = ok(
        &mut client,
        "configure_dhcp_server",
        json!({ "device": "E2E-SRV", "pools": [{ "name": "serverPool", "gateway": "192.168.60.1",
                "start_ip": "192.168.60.100", "mask": "255.255.255.0", "dns": "192.168.60.5", "max_users": 20 }] }),
    )
    .await;
    assert_eq!(dhcp["enabled"], true);
    ok(
        &mut client,
        "configure_dns_server",
        json!({ "device": "E2E-SRV", "records": [{ "name": "www.e2e.bo", "type": "A", "value": "192.168.60.5" }] }),
    )
    .await;
    ok(&mut client, "fast_forward", json!({})).await;
    let lease = ok(
        &mut client,
        "configure_host",
        json!({ "device": PC_A, "dhcp": true }),
    )
    .await;
    let leased = lease["ip"].as_str().unwrap_or_default().to_owned();
    assert!(leased.starts_with("192.168.60."), "{lease}");
    let resolved = ok(
        &mut client,
        "run_host_command",
        json!({ "device": PC_A, "command": "ping -n 1 www.e2e.bo" }),
    )
    .await;
    assert!(
        resolved["output"]
            .as_str()
            .unwrap()
            .contains("192.168.60.5"),
        "{resolved}"
    );
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn changes_preferences_and_restores_them() {
    let mut client = live_client().await;
    let before = ok(&mut client, "get_preferences", json!({})).await;
    let lights = before["values"]["show_link_lights"].as_bool().unwrap();
    let after = ok(
        &mut client,
        "set_preferences",
        json!({ "values": { "show_link_lights": !lights } }),
    )
    .await;
    assert_eq!(after["values"]["show_link_lights"], !lights);
    let restored = ok(
        &mut client,
        "set_preferences",
        json!({ "values": { "show_link_lights": lights } }),
    )
    .await;
    assert_eq!(restored, before);
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn watches_devices_being_added() {
    let mut watcher = live_client().await;
    let mut actor = live_client().await;
    remove_leftovers(&mut actor).await;
    let listen = ok(
        &mut watcher,
        "watch_events",
        json!({ "class": "LogicalWorkspace", "events": ["deviceAdded"], "seconds": 6, "max_events": 1 }),
    );
    let act = async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        ok(
            &mut actor,
            "add_device",
            json!({ "model": "2911", "name": ROUTER }),
        )
        .await
    };
    let (seen, _) = tokio::join!(listen, act);
    assert_eq!(seen["events"][0]["event"], "deviceAdded", "{seen}");
    assert_eq!(seen["events"][0]["args"][1], "2911");
    remove_leftovers(&mut actor).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer 9.0.1 on macOS with the pktctl ExApp registered, including FILE"]
async fn reads_a_sample_activity() {
    const SAMPLE: &str = "/Applications/Cisco Packet Tracer 9.0.1/Cisco Packet Tracer 9.0.1.app/\
Contents/saves/06 Industrial -  OT/Industrial Control Systems/PLC - LadderLogic/latching-with-plc.pka";
    let mut client = live_client().await;
    let scratch = std::env::temp_dir().join(format!("pktctl-activity-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let copy = scratch.join("latching.pka");
    std::fs::copy(SAMPLE, &copy).unwrap();
    let saved = scratch.join("before.pkt").display().to_string();
    ok(&mut client, "save_network", json!({ "path": saved })).await;

    ok(
        &mut client,
        "open_network",
        json!({ "path": copy.display().to_string() }),
    )
    .await;
    let status = ok(&mut client, "activity_status", json!({})).await;
    assert_eq!(status["is_activity"], true, "{status}");
    let page = ok(&mut client, "activity_instructions", json!({})).await;
    assert!(
        page["text"].as_str().unwrap().contains("Latching"),
        "{page}"
    );
    ok(&mut client, "check_activity", json!({})).await;

    ok(&mut client, "open_network", json!({ "path": saved })).await;
    let plain = ok(&mut client, "activity_status", json!({})).await;
    assert_eq!(plain["is_activity"], false);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[tokio::test]
#[ignore = "needs a visible Packet Tracer window and Screen Recording permission on macOS"]
async fn captures_the_physical_workspace() {
    let mut client = live_client().await;
    for view in ["physical", "physical_rack", "window"] {
        let shot = client
            .call_tool("screenshot", json!({ "view": view }))
            .await;
        assert_eq!(
            shot["content"][0]["mimeType"], "image/png",
            "{view}: {shot}"
        );
    }
    let mode = ok(
        &mut client,
        "call_ipc",
        json!({ "from": "appWindow", "steps": [{ "method": "isPhysicalMode" }] }),
    )
    .await;
    assert_eq!(mode["value"], false, "the logical view is restored");
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer; power cycles every device in the open network"]
async fn power_cycles_everything_without_a_dialog() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "2911", "name": ROUTER }),
    )
    .await;
    ok(
        &mut client,
        "configure_ios",
        json!({ "device": ROUTER, "commands": ["hostname SAVED"], "save": true }),
    )
    .await;
    ok(
        &mut client,
        "configure_ios",
        json!({ "device": ROUTER, "commands": ["hostname UNSAVED"] }),
    )
    .await;

    let cycled = ok(&mut client, "power_cycle_all", json!({})).await;
    assert!(
        cycled["devices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == ROUTER),
        "{cycled}"
    );
    let prompt = ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "show running-config | include hostname" }),
    )
    .await;
    assert!(
        prompt["output"]
            .as_str()
            .unwrap()
            .contains("hostname SAVED"),
        "{prompt}"
    );
    ok(&mut client, "status", json!({})).await;
    remove_leftovers(&mut client).await;
}

const SERVER: &str = "E2E-SRV";
const SERVER_IP: &str = "192.168.70.5";

async fn office(client: &mut McpClient) {
    remove_leftovers(client).await;
    for (model, name) in [
        ("Server-PT", SERVER),
        ("2960-24TT", SWITCH),
        ("PC-PT", PC_A),
    ] {
        ok(
            client,
            "add_device",
            json!({ "model": model, "name": name }),
        )
        .await;
    }
    for (device, port) in [(SERVER, "FastEthernet0/1"), (PC_A, "FastEthernet0/2")] {
        ok(
            client,
            "connect",
            json!({ "device_a": SWITCH, "port_a": port, "device_b": device, "port_b": "FastEthernet0" }),
        )
        .await;
    }
    for (device, ip) in [(SERVER, SERVER_IP), (PC_A, "192.168.70.20")] {
        ok(
            client,
            "configure_host",
            json!({ "device": device, "ip": ip, "mask": "255.255.255.0", "dns": SERVER_IP }),
        )
        .await;
    }
    ok(
        client,
        "configure_dns_server",
        json!({ "device": SERVER, "records": [{ "name": "www.e2e.bo", "type": "A", "value": SERVER_IP }] }),
    )
    .await;
    ok(
        client,
        "set_web_page",
        json!({ "device": SERVER, "url": "index.html", "contents": "<h1>E2E</h1><p>pagina</p>" }),
    )
    .await;
    for user in ["ana", "luis"] {
        ok(
            client,
            "add_server_user",
            json!({ "device": SERVER, "service": "email", "username": user, "password": "cisco", "domain": "e2e.bo" }),
        )
        .await;
    }
    ok(client, "fast_forward", json!({})).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn browses_and_mails_between_hosts() {
    let mut client = live_client().await;
    office(&mut client).await;

    let page = ok(
        &mut client,
        "browse_web",
        json!({ "device": PC_A, "url": "www.e2e.bo" }),
    )
    .await;
    assert_eq!(
        (page["status"].as_str(), page["server"].as_str()),
        (Some("ok"), Some(SERVER_IP)),
        "{page}"
    );
    assert_eq!(page["text"], "E2E\npagina");
    let unknown = ok(
        &mut client,
        "browse_web",
        json!({ "device": PC_A, "url": "www.nadie.bo" }),
    )
    .await;
    assert_eq!(unknown["status"], "host_not_found", "{unknown}");

    for (device, user) in [(PC_A, "ana"), (SERVER, "luis")] {
        ok(
            &mut client,
            "configure_email",
            json!({ "device": device, "name": user, "email": format!("{user}@e2e.bo"), "username": user,
                    "password": "cisco", "incoming_server": SERVER_IP, "outgoing_server": SERVER_IP }),
        )
        .await;
    }
    let mail =
        json!({ "device": PC_A, "to": "luis@e2e.bo", "subject": "Informe", "body": "Adjunto" });
    let mut sent = client.call_tool("send_email", mail.clone()).await;
    if sent["isError"] == true {
        sent = client.call_tool("send_email", mail).await;
    }
    assert_ne!(sent["isError"], true, "{sent}");
    let inbox = ok(&mut client, "receive_email", json!({ "device": SERVER })).await;
    assert_eq!(inbox["mails"][0]["subject"], "Informe", "{inbox}");
    assert_eq!(inbox["mails"][0]["from"], "ana@e2e.bo");
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn configures_host_files_ipv6_and_firewall() {
    let mut client = live_client().await;
    office(&mut client).await;

    ok(
        &mut client,
        "host_files",
        json!({ "device": PC_A, "action": "write", "name": "e2e.txt", "text": "VLAN 10" }),
    )
    .await;
    let read = ok(
        &mut client,
        "host_files",
        json!({ "device": PC_A, "action": "read", "name": "e2e.txt" }),
    )
    .await;
    assert_eq!(read["text"], "VLAN 10");
    let dir = ok(
        &mut client,
        "run_host_command",
        json!({ "device": PC_A, "command": "dir" }),
    )
    .await;
    assert!(dir["output"].as_str().unwrap().contains("e2e.txt"), "{dir}");
    ok(
        &mut client,
        "host_files",
        json!({ "device": PC_A, "action": "delete", "name": "e2e.txt" }),
    )
    .await;

    let ipv6 = ok(
        &mut client,
        "configure_host_ipv6",
        json!({ "device": PC_A, "address": "2001:db8:70::20/64", "gateway": "fe80::1" }),
    )
    .await;
    assert_eq!(ipv6["addresses"], json!(["2001:db8:70::20/64"]));
    let shown = ok(
        &mut client,
        "run_host_command",
        json!({ "device": PC_A, "command": "ipconfig" }),
    )
    .await;
    assert!(
        shown["output"]
            .as_str()
            .unwrap()
            .contains("2001:DB8:70::20"),
        "{shown}"
    );
    let firewall = ok(
        &mut client,
        "set_host_firewall",
        json!({ "device": PC_A, "ipv4": true, "ipv6": true }),
    )
    .await;
    assert_eq!(
        (firewall["ipv4"].as_bool(), firewall["ipv6"].as_bool()),
        (Some(true), Some(true))
    );
    let blocked = ok(
        &mut client,
        "set_host_firewall",
        json!({ "device": PC_A, "ipv4": true, "add_rules": [{ "action": "deny", "protocol": "icmp" }] }),
    )
    .await;
    assert_eq!(
        blocked["ipv4_rules"],
        json!(["deny icmp any any"]),
        "{blocked}"
    );
    let refused = ok(
        &mut client,
        "run_host_command",
        json!({ "device": SERVER, "command": format!("ping -n 2 {}", "192.168.70.20") }),
    )
    .await;
    assert!(
        !refused["output"].as_str().unwrap().contains("Reply from"),
        "the firewall rule should drop the ping: {refused}"
    );
    let cleared = ok(
        &mut client,
        "set_host_firewall",
        json!({ "device": PC_A, "ipv4": false, "ipv6": false,
                "remove_rules": [{ "action": "deny", "protocol": "icmp" }] }),
    )
    .await;
    assert!(
        cleared["ipv4_rules"].as_array().unwrap().is_empty(),
        "{cleared}"
    );
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn connects_the_vpn_client() {
    let mut client = live_client().await;
    office(&mut client).await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "2811", "name": ROUTER }),
    )
    .await;
    ok(
        &mut client,
        "connect",
        json!({ "device_a": ROUTER, "port_a": "FastEthernet0/0", "device_b": SWITCH, "port_b": "FastEthernet0/3" }),
    )
    .await;
    let configured = ok(
        &mut client,
        "configure_ios",
        json!({ "device": ROUTER, "commands": [
            "interface FastEthernet0/0", "ip address 192.168.70.2 255.255.255.0", "no shutdown", "exit",
            "aaa new-model", "aaa authentication login VPNAUTH local",
            "aaa authorization network VPNGRP local", "username vpnuser password vpnpass",
            "ip local pool VPNPOOL 10.70.0.10 10.70.0.50",
            "crypto isakmp policy 10", "encryption aes 256", "hash sha", "authentication pre-share",
            "group 2", "exit",
            "crypto isakmp client configuration group VPNGROUP", "key vpnkey", "pool VPNPOOL", "exit",
            "crypto ipsec transform-set TS esp-aes esp-sha-hmac",
            "crypto dynamic-map DYN 10", "set transform-set TS", "reverse-route", "exit",
            "crypto map CMAP client authentication list VPNAUTH",
            "crypto map CMAP isakmp authorization list VPNGRP",
            "crypto map CMAP client configuration address respond",
            "crypto map CMAP 10 ipsec-isakmp dynamic DYN",
            "interface FastEthernet0/0", "crypto map CMAP"
        ] }),
    )
    .await;
    assert_eq!(configured["completed"], true, "{configured}");
    ok(&mut client, "fast_forward", json!({})).await;
    ping(&mut client, PC_A, "192.168.70.2").await;

    let up = ok(
        &mut client,
        "vpn_client",
        json!({ "device": PC_A, "action": "connect", "server": "192.168.70.2", "group": "VPNGROUP",
                "group_key": "vpnkey", "username": "vpnuser", "password": "vpnpass" }),
    )
    .await;
    assert_eq!(up["connected"], true, "{up}");
    assert!(
        up["tunnel_ip"].as_str().unwrap().starts_with("10.70.0."),
        "{up}"
    );
    let down = ok(
        &mut client,
        "vpn_client",
        json!({ "device": PC_A, "action": "disconnect" }),
    )
    .await;
    assert_eq!(down["connected"], false);
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn answers_console_questions() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "2911", "name": ROUTER }),
    )
    .await;
    let asked = ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "copy running-config startup-config" }),
    )
    .await;
    assert_eq!(
        asked["question"], "Destination filename [startup-config]?",
        "{asked}"
    );
    let blocked = client
        .call_tool(
            "run_cli",
            json!({ "device": ROUTER, "command": "show clock" }),
        )
        .await;
    assert_eq!(blocked["isError"], true, "{blocked}");
    let saved = ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "", "mode": "current" }),
    )
    .await;
    assert_eq!(saved["finished"], true, "{saved}");

    let reload = ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "reload" }),
    )
    .await;
    assert_eq!(reload["question"], "Proceed with reload? [confirm]");
    ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "", "mode": "current" }),
    )
    .await;
    let uptime = ok(
        &mut client,
        "run_cli",
        json!({ "device": ROUTER, "command": "show version | include uptime", "timeout_secs": 60 }),
    )
    .await;
    assert!(
        uptime["output"].as_str().unwrap().contains("uptime is"),
        "{uptime}"
    );
    remove_leftovers(&mut client).await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn furnishes_a_room_and_arranges_it() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    for model in ["2911", "2960-24TT"] {
        let name = if model == "2911" { ROUTER } else { SWITCH };
        ok(
            &mut client,
            "add_device",
            json!({ "model": model, "name": name }),
        )
        .await;
    }
    let locations = ok(&mut client, "list_locations", json!({})).await;
    let closet = locations["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|location| location["kind"] == "wiring_closet")
        .and_then(|location| location["path"].as_str())
        .expect("every network has a wiring closet")
        .to_owned();
    let closet = closet.as_str();
    let table = ok(
        &mut client,
        "add_location",
        json!({ "kind": "table", "inside": closet, "name": "E2E Mesa", "x": 5, "y": 5 }),
    )
    .await;
    assert_eq!(table["location"]["kind"], "stackable_table", "{table}");
    assert!(table["file"].is_string(), "furniture goes through the file");

    let onto = ok(
        &mut client,
        "arrange_devices",
        json!({ "location": format!("{closet}/E2E Mesa"), "devices": [ROUTER, SWITCH],
                "columns": 2, "spacing_x": 4, "start_x": 2, "start_y": 2 }),
    )
    .await;
    assert_eq!(onto["devices"][0]["device"], ROUTER, "{onto}");
    let locations = ok(&mut client, "list_locations", json!({})).await;
    let mesa = locations["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|location| location["name"] == "E2E Mesa")
        .unwrap();
    let devices = mesa["devices"].as_array().unwrap();
    assert!(
        devices.iter().any(|device| device == ROUTER) && devices.iter().any(|d| d == SWITCH),
        "{mesa}"
    );

    let paper = ok(
        &mut client,
        "set_background",
        json!({ "location": closet, "image": "grid_50x50", "tiled": true }),
    )
    .await;
    assert_eq!(paper["image"], "../art/Background/grid_50x50.png");

    remove_leftovers(&mut client).await;
    ok(
        &mut client,
        "remove_location",
        json!({ "path": format!("{closet}/E2E Mesa") }),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer"]
async fn removes_physical_locations() {
    let mut client = live_client().await;
    remove_leftovers(&mut client).await;
    loop {
        let existing = ok(&mut client, "list_locations", json!({})).await;
        let Some(stale) = existing["locations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|location| location["path"].as_str())
            .find(|path| path.starts_with("E2E") && !path.contains('/'))
            .map(str::to_owned)
        else {
            break;
        };
        ok(&mut client, "remove_location", json!({ "path": stale })).await;
    }
    let city = ok(&mut client, "add_location", json!({ "kind": "city" })).await;
    let city_path = city["location"]["path"].as_str().unwrap().to_owned();
    let named = ok(
        &mut client,
        "rename_location",
        json!({ "path": city_path, "name": "E2E Ciudad" }),
    )
    .await;
    assert_eq!(named["location"]["path"], "E2E Ciudad");
    let closet = ok(
        &mut client,
        "add_location",
        json!({ "kind": "wiring_closet", "inside": "E2E Ciudad" }),
    )
    .await;
    ok(
        &mut client,
        "add_device",
        json!({ "model": "PC-PT", "name": PC_A }),
    )
    .await;
    ok(
        &mut client,
        "move_to_location",
        json!({ "device": PC_A, "into": closet["location"]["path"] }),
    )
    .await;
    let busy = client
        .call_tool("remove_location", json!({ "path": "E2E Ciudad" }))
        .await;
    assert_eq!(busy["isError"], true, "{busy}");
    remove_leftovers(&mut client).await;

    let removed = ok(
        &mut client,
        "remove_location",
        json!({ "path": "E2E Ciudad" }),
    )
    .await;
    assert_eq!(removed["removed"], "E2E Ciudad");
    let locations = ok(&mut client, "list_locations", json!({})).await;
    assert!(
        !locations["locations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|location| location["path"].as_str().unwrap().starts_with("E2E")),
        "{locations}"
    );
}
