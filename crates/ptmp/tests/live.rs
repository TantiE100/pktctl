//! Runs against a real Packet Tracer. Enable with `make e2e-live`; see docs/development.md.

use ptmp::{Call, Credentials, Session, SessionConfig, Value};

const ROUTER: i32 = 0;

fn live_config() -> SessionConfig {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"));
    SessionConfig::new(
        std::env::var("PKTCTL_ADDR").unwrap_or_else(|_| "127.0.0.1:39000".into()),
        Credentials {
            app_id: var("PKTCTL_APP_ID"),
            secret: var("PKTCTL_SECRET"),
        },
    )
}

fn logical_workspace() -> Call {
    Call::root("appWindow")
        .method("getActiveWorkspace", [])
        .method("getLogicalWorkspace", [])
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn handshake_and_read_only_calls() {
    let session = Session::connect(&live_config()).await.unwrap();
    assert!(
        session
            .pt_version()
            .is_some_and(|version| version.starts_with('9'))
    );

    let count = session
        .call(Call::root("network").method("getDeviceCount", []))
        .await
        .unwrap();
    assert!(count.as_i64().is_some_and(|count| count >= 0));

    let missing = session
        .call(
            Call::root("network")
                .method("getDevice", [Value::qstring("pktctl-does-not-exist")])
                .method("getName", []),
        )
        .await;
    assert!(missing.is_err());
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn non_ascii_names_survive_the_length_prefix() {
    let session = Session::connect(&live_config()).await.unwrap();
    let created = session
        .call(logical_workspace().method(
            "addDevice",
            [
                Value::Int(ROUTER),
                Value::string("2911"),
                Value::Double(120.0),
                Value::Double(120.0),
            ],
        ))
        .await
        .unwrap();
    let original = created.as_str().unwrap().to_owned();
    let unicode = "Oficiña-Ñandú-→";

    let device = |name: &str| Call::root("network").method("getDevice", [Value::qstring(name)]);
    session
        .call(device(&original).method("setName", [Value::qstring(unicode)]))
        .await
        .unwrap();
    let read_back = session
        .call(device(unicode).method("getName", []))
        .await
        .unwrap();
    session
        .call(logical_workspace().method("removeDevice", [Value::qstring(unicode)]))
        .await
        .unwrap();

    assert_eq!(read_back.as_str(), Some(unicode));
}

#[tokio::test]
#[ignore = "needs a running Packet Tracer with the pktctl ExApp registered"]
async fn workspace_image_arrives_as_raw_png_bytes() {
    let session = Session::connect(&live_config()).await.unwrap();
    let image = session
        .call(logical_workspace().method("getWorkspaceImage", [Value::qstring("PNG")]))
        .await
        .unwrap()
        .into_bytes()
        .unwrap();
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
}
