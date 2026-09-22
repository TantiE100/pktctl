const EMPTY_NETWORK: &[u8] = include_bytes!("../assets/empty-9.0.1.pkt");

#[test]
fn decodes_a_file_saved_by_packet_tracer() {
    let xml = pktfile::decode(EMPTY_NETWORK).unwrap();
    assert!(xml.contains("<VERSION>9.0.1.0858</VERSION>"));
    assert!(xml.contains("<PHYSICALWORKSPACE>"));
    assert!(xml.contains("Main Wiring Closet"));
}

#[test]
fn re_encoded_files_decode_to_the_same_xml() {
    let xml = pktfile::decode(EMPTY_NETWORK).unwrap();
    let again = pktfile::decode(&pktfile::encode(&xml).unwrap()).unwrap();
    assert_eq!(again, xml);
}
