#![allow(dead_code)]

use std::{process::Stdio, time::Duration};

use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct McpClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl McpClient {
    pub async fn spawn_as(addr: &str, app_id: &str, secret: &str) -> Self {
        Self::spawn_with(addr, app_id, secret, &[]).await
    }

    pub async fn spawn_with(
        addr: &str,
        app_id: &str,
        secret: &str,
        extra: &[(&str, &str)],
    ) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pktctl"))
            .env("PKTCTL_ADDR", addr)
            .env("PKTCTL_APP_ID", app_id)
            .env("PKTCTL_SECRET", secret)
            .envs(extra.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("pktctl binary starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut client = Self {
            _child: child,
            stdin,
            stdout,
            next_id: 1,
        };
        client.initialize().await;
        client
    }

    async fn initialize(&mut self) {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "pktctl-e2e", "version": "0" }
                }),
            )
            .await;
        assert_eq!(result["serverInfo"]["name"], "pktctl");
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }

    pub async fn request(&mut self, method: &str, params: Json) -> Json {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line = timeout(RESPONSE_TIMEOUT, self.stdout.next_line())
                .await
                .expect("pktctl answers in time")
                .unwrap()
                .expect("pktctl keeps stdout open");
            let message: Json = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                assert!(message.get("error").is_none(), "json-rpc error: {message}");
                return message["result"].clone();
            }
        }
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Json) -> Json {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    async fn send(&mut self, message: Json) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }
}
