//! Runs against a file saved by a real Packet Tracer, which this repository does not ship.
//! Point `PKTCTL_TEST_PKT` at any `.pkt` and run `make e2e-live`; see docs/development.md.

fn saved_by_packet_tracer() -> Vec<u8> {
    let path = std::env::var("PKTCTL_TEST_PKT").expect("PKTCTL_TEST_PKT must be set");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path} could not be read: {error}"))
}

#[test]
#[ignore = "needs a .pkt saved by Packet Tracer"]
fn decodes_a_file_saved_by_packet_tracer() {
    let xml = pktfile::decode(&saved_by_packet_tracer()).unwrap();
    assert!(xml.contains("<VERSION>"));
    assert!(xml.contains("<PHYSICALWORKSPACE>"));
}

#[test]
#[ignore = "needs a .pkt saved by Packet Tracer"]
fn re_encoded_files_decode_to_the_same_xml() {
    let xml = pktfile::decode(&saved_by_packet_tracer()).unwrap();
    let again = pktfile::decode(&pktfile::encode(&xml).unwrap()).unwrap();
    assert_eq!(again, xml);
}
