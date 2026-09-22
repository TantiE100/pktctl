//! Prints the XML inside a Packet Tracer file: `cargo run -p pktfile --example pkt2xml -- file.pkt`.

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: pkt2xml <file.pkt|file.pka>");
        return ExitCode::FAILURE;
    };
    let decoded = fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| pktfile::decode(&bytes).map_err(|error| error.to_string()));
    match decoded {
        Ok(xml) => {
            println!("{xml}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{path}: {error}");
            ExitCode::FAILURE
        }
    }
}
