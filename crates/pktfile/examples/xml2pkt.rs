//! Packs XML back into a Packet Tracer file, the other half of `pkt2xml`:
//! `cargo run -p pktfile --example xml2pkt -- edited.xml file.pkt`.

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let (Some(xml_path), Some(pkt_path)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: xml2pkt <file.xml> <file.pkt>");
        return ExitCode::FAILURE;
    };
    let written = fs::read_to_string(&xml_path)
        .map_err(|error| format!("{xml_path}: {error}"))
        .and_then(|xml| pktfile::encode(&xml).map_err(|error| format!("{xml_path}: {error}")))
        .and_then(|bytes| {
            fs::write(&pkt_path, &bytes)
                .map(|()| bytes.len())
                .map_err(|error| format!("{pkt_path}: {error}"))
        });
    match written {
        Ok(bytes) => {
            eprintln!("{pkt_path}: {bytes} bytes");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
