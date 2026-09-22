use std::time::Duration;

use ptmp::{
    Call, Credentials, Error, Event, Session, SessionConfig, Subscription, Value,
    fake::{FAKE_PT_VERSION, FakePt, Reply},
};

fn credentials() -> Credentials {
    Credentials {
        app_id: "dev.pktctl.test".into(),
        secret: "s3cret".into(),
    }
}

fn config_for(pt: &FakePt) -> SessionConfig {
    SessionConfig::new(pt.addr().to_string(), credentials())
}

fn device_count() -> Call {
    Call::root("network").method("getDeviceCount", [])
}

async fn eleven_devices() -> FakePt {
    FakePt::start(credentials(), |_| Value::Int(11).into())
        .await
        .unwrap()
}

#[tokio::test]
async fn negotiates_and_reports_packet_tracer_version() {
    let pt = eleven_devices().await;
    let session = Session::connect(&config_for(&pt)).await.unwrap();
    assert_eq!(session.pt_version(), Some(FAKE_PT_VERSION));
}

#[tokio::test]
async fn wrong_secret_is_rejected_with_actionable_error() {
    let pt = eleven_devices().await;
    let mut config = config_for(&pt);
    config.credentials.secret = "wrong".into();
    let error = Session::connect(&config).await.unwrap_err();
    assert!(matches!(error, Error::AuthRejected { ref app_id } if app_id == "dev.pktctl.test"));
}

#[tokio::test]
async fn unreachable_packet_tracer_reports_address() {
    let config = SessionConfig::new("127.0.0.1:1", credentials());
    let error = Session::connect(&config).await.unwrap_err();
    assert!(matches!(error, Error::Connect { ref addr, .. } if addr == "127.0.0.1:1"));
}

#[tokio::test]
async fn call_returns_typed_value_and_reaches_server_intact() {
    let pt = eleven_devices().await;
    let session = Session::connect(&config_for(&pt)).await.unwrap();

    assert_eq!(session.call(device_count()).await.unwrap(), Value::Int(11));
    assert_eq!(pt.calls(), vec![device_count()]);
}

#[tokio::test]
async fn remote_errors_surface_class_and_message() {
    let pt = FakePt::start(credentials(), |_| {
        Reply::error("Device", "IPC Cache entry: ")
    })
    .await
    .unwrap();
    let session = Session::connect(&config_for(&pt)).await.unwrap();

    let error = session.call(device_count()).await.unwrap_err();
    assert!(matches!(error, Error::Remote { ref class, .. } if class == "Device"));
}

#[tokio::test]
async fn concurrent_calls_are_matched_to_their_own_replies() {
    let pt = FakePt::start(credentials(), |call| {
        let name = call.steps()[1].args[0]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Value::string(format!("model-of-{name}")).into()
    })
    .await
    .unwrap();
    let session = Session::connect(&config_for(&pt)).await.unwrap();

    let calls = (0..64).map(|index| {
        let session = session.clone();
        async move {
            let call = Call::root("network")
                .method("getDevice", [Value::qstring(format!("R{index}"))])
                .method("getModel", []);
            (index, session.call(call).await.unwrap())
        }
    });
    for (index, value) in futures::future::join_all(calls).await {
        assert_eq!(value.as_str(), Some(format!("model-of-R{index}").as_str()));
    }
}

#[tokio::test]
async fn silent_server_times_out_the_call() {
    let pt = FakePt::start(credentials(), |_| Reply::Silence)
        .await
        .unwrap();
    let mut config = config_for(&pt);
    config.call_timeout = Duration::from_millis(100);
    let session = Session::connect(&config).await.unwrap();

    let error = session.call(device_count()).await.unwrap_err();
    assert!(matches!(error, Error::Timeout(_)));
}

#[tokio::test]
async fn subscriptions_are_sent_and_events_are_delivered() {
    let pt = eleven_devices().await;
    let session = Session::connect(&config_for(&pt)).await.unwrap();
    let mut events = session.events();

    let subscription = Subscription::to("Device", "{r1}", "nameChanged");
    session.subscribe(subscription.clone()).unwrap();
    session.call(device_count()).await.unwrap();
    assert_eq!(pt.subscriptions(), vec![subscription]);

    let event = Event {
        token: "1".into(),
        class: "Device".into(),
        object_uuid: "{r1}".into(),
        name: "nameChanged".into(),
        args: vec![Value::qstring("R1X"), Value::qstring("R1")],
    };
    pt.emit(event.clone());
    let received = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, event);
}

#[tokio::test]
async fn calls_after_close_fail_fast() {
    let pt = eleven_devices().await;
    let session = Session::connect(&config_for(&pt)).await.unwrap();

    session.close();
    assert!(session.is_closed());
    assert!(matches!(
        session.call(device_count()).await,
        Err(Error::Closed)
    ));
}
