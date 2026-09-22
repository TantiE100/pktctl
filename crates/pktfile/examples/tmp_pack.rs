fn main() {
    let mut args = std::env::args().skip(1);
    let xml = std::fs::read_to_string(args.next().unwrap()).unwrap();
    std::fs::write(args.next().unwrap(), pktfile::encode(&xml).unwrap()).unwrap();
}
