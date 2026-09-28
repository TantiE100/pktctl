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
    output_schemas: Option<Json>,
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
            output_schemas: None,
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
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        if result["isError"] != true
            && let Some(content) = result.get("structuredContent")
        {
            let schema = self.output_schema(name).await;
            let mut missing = Vec::new();
            missing_required(content, &schema, &schema, "", &mut missing);
            assert!(
                missing.is_empty(),
                "{name} returned content its outputSchema rejects, missing required {missing:?}: {content}"
            );
        }
        result
    }

    async fn output_schema(&mut self, name: &str) -> Json {
        if self.output_schemas.is_none() {
            let listing = self.request("tools/list", json!({})).await;
            let schemas = listing["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| {
                    (
                        tool["name"].as_str().unwrap().to_owned(),
                        tool["outputSchema"].clone(),
                    )
                })
                .collect();
            self.output_schemas = Some(Json::Object(schemas));
        }
        self.output_schemas.as_ref().unwrap()[name].clone()
    }

    async fn send(&mut self, message: Json) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }
}

fn missing_required(
    value: &Json,
    schema: &Json,
    root: &Json,
    path: &str,
    missing: &mut Vec<String>,
) {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.rsplit('/').next().unwrap();
        let target = if root["$defs"][name].is_null() {
            &root["definitions"][name]
        } else {
            &root["$defs"][name]
        };
        return missing_required(value, target, root, path, missing);
    }
    if let Some(object) = value.as_object() {
        for key in schema["required"].as_array().into_iter().flatten() {
            let key = key.as_str().unwrap();
            if !object.contains_key(key) {
                missing.push(format!("{path}/{key}"));
            }
        }
        if let Some(properties) = schema["properties"].as_object() {
            for (key, property) in properties {
                if let Some(child) = object.get(key) {
                    missing_required(child, property, root, &format!("{path}/{key}"), missing);
                }
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            missing_required(item, items, root, &format!("{path}/{index}"), missing);
        }
    }
    for part in schema["allOf"].as_array().into_iter().flatten() {
        missing_required(value, part, root, path, missing);
    }
}
