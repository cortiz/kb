//! Integration tests for Runbook support in Model Context Protocol (MCP) server.

mod support;

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use support::{NOWHERE, pty};

fn kb_mcp_cmd_with_args(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(pty::KB);
    cmd.arg("--root").arg(root).arg("mcp");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.env("HOME", NOWHERE)
        .env("XDG_CONFIG_HOME", format!("{NOWHERE}/config"));
    cmd
}

struct McpClient {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

impl McpClient {
    fn spawn(mut cmd: Command) -> Self {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        Self { child, reader }
    }

    fn send(&mut self, msg: Value) {
        let line = serde_json::to_string(&msg).unwrap();
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", line).unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(!line.trim().is_empty(), "expected response from kb mcp");
        serde_json::from_str(line.trim()).unwrap()
    }

    fn init(&mut self) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "claude-code",
                    "version": "1.0.0"
                }
            }
        }));
        let resp = self.recv();
        assert_eq!(resp["id"], 1);

        self.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    fn close(mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

fn fixture_git_kb() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kb");
    fs::create_dir_all(root.join("sumo")).unwrap();
    fs::create_dir_all(root.join("cmd")).unwrap();
    fs::create_dir_all(root.join("secret")).unwrap();
    fs::create_dir_all(root.join("runbook")).unwrap();

    fs::write(
        root.join("sumo/5xx.md"),
        r#"---
kb_format: 1
type: sumo
title: 5xx errors by service
vars:
  env: { kind: enum, options: [dev, stage, prod], default: stage }
  minutes: { kind: number, default: 30 }
---

```sumo
_sourceCategory=api/{{env}} status >= 500 | timeslice {{minutes}}m
```
"#,
    )
    .unwrap();

    fs::write(
        root.join("cmd/deploy.md"),
        r#"---
kb_format: 1
type: cmd
title: Deploy service
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text }
---

```sh
deploy.sh --env {{env}} --version {{version}}
```
"#,
    )
    .unwrap();

    fs::write(
        root.join("secret/db.md"),
        r#"---
kb_format: 1
type: cmd
title: DB Access
tags: [private]
---

```sh
mysql -u root
```
"#,
    )
    .unwrap();

    fs::write(
        root.join("runbook/deploy.md"),
        r#"---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, default: v1.0.0 }
steps:
  - id: baseline
    title: Check the current error rate
    entry: sumo/5xx
    vars: { minutes: 15 }
  - id: private-step
    title: Internal DB Access
    entry: secret/db
  - id: deploy
    title: Deploy {{version}} to {{env}}
    entry: cmd/deploy
    confirm: true
  - id: verify
    title: Verify deployment
    do: Check dashboard manually
    optional: true
---

Owner: platform security.
"#,
    )
    .unwrap();

    let _ = Command::new("git").args(["init"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.name", "Test Agent"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.email", "agent@example.com"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["add", "-A"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["commit", "-m", "initial commit"]).current_dir(&root).output().unwrap();

    (temp, root)
}

#[test]
fn test_mcp_runbook_get_unresolved_and_hidden_steps() {
    let (_temp, root) = fixture_git_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &[]));
    client.init();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "runbook/deploy",
                "resolve": false
            }
        }
    }));

    let resp = client.recv();
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);

    let parsed: Value = serde_json::from_str(
        resp["result"]["content"][0]["text"].as_str().unwrap()
    ).unwrap();

    assert_eq!(parsed["id"], "runbook/deploy");
    assert_eq!(parsed["type"], "runbook");
    assert_eq!(parsed["title"], "Deploy authorizer to prod");

    let steps = parsed["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 4);

    // Step 1: baseline
    assert_eq!(steps[0]["id"], "baseline");
    assert_eq!(steps[0]["entry"], "sumo/5xx");
    assert_eq!(steps[0]["vars"]["minutes"], 15);

    // Step 2: private-step must be hidden because secret/db is tagged "private"
    assert_eq!(steps[1]["hidden"], true);
    assert_eq!(steps[1]["position"], 2);
    assert!(steps[1].get("id").is_none());
    assert!(steps[1].get("title").is_none());
    assert!(steps[1].get("entry").is_none());

    // Step 3: deploy
    assert_eq!(steps[2]["id"], "deploy");
    assert_eq!(steps[2]["entry"], "cmd/deploy");
    assert_eq!(steps[2]["confirm"], true);

    // Step 4: manual step
    assert_eq!(steps[3]["id"], "verify");
    assert_eq!(steps[3]["do"], "Check dashboard manually");
    assert_eq!(steps[3]["optional"], true);

    // Binding table check
    let binding_table = parsed["binding_table"].as_array().expect("binding_table");
    assert!(!binding_table.is_empty());
    let baseline_row = binding_table.iter().find(|r| r["step_id"] == "baseline").unwrap();
    assert_eq!(baseline_row["entry_id"], "sumo/5xx");

    client.close();
}

#[test]
fn test_mcp_runbook_get_resolved() {
    let (_temp, root) = fixture_git_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &[]));
    client.init();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "runbook/deploy",
                "resolve": true,
                "vars": {
                    "env": "prod",
                    "version": "v2.14.0"
                }
            }
        }
    }));

    let resp = client.recv();
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);

    let parsed: Value = serde_json::from_str(
        resp["result"]["content"][0]["text"].as_str().unwrap()
    ).unwrap();

    let steps = parsed["steps"].as_array().expect("steps array");
    // Step 1: baseline resolved payload
    assert!(steps[0]["resolved_payload"].as_str().unwrap().contains("_sourceCategory=api/prod"));
    assert!(steps[0]["resolved_payload"].as_str().unwrap().contains("timeslice 15m"));

    // Step 2: still hidden
    assert_eq!(steps[1]["hidden"], true);
    assert_eq!(steps[1]["position"], 2);

    // Step 3: deploy title interpolated and payload resolved
    assert_eq!(steps[2]["title"], "Deploy v2.14.0 to prod");
    assert!(steps[2]["resolved_payload"].as_str().unwrap().contains("deploy.sh --env prod --version v2.14.0"));

    client.close();
}

#[test]
fn test_mcp_entry_used_in_and_update() {
    let (_temp, root) = fixture_git_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &["--allow-write"]));
    client.init();

    // 1. Get sumo/5xx -> must include used_in with runbook/deploy
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/5xx"
            }
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 4);
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value = serde_json::from_str(
        resp["result"]["content"][0]["text"].as_str().unwrap()
    ).unwrap();
    let used_in = parsed["used_in"].as_array().expect("used_in array");
    assert_eq!(used_in.len(), 1);
    assert_eq!(used_in[0]["id"], "runbook/deploy");
    assert_eq!(used_in[0]["title"], "Deploy authorizer to prod");

    let version = parsed["version"].as_str().unwrap().to_string();

    // 2. Update sumo/5xx -> update response must also include used_in
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "update",
            "arguments": {
                "id": "sumo/5xx",
                "version": version,
                "changes": {
                    "add_tags": ["monitored"]
                }
            }
        }
    }));
    let update_resp = client.recv();
    assert_eq!(update_resp["id"], 5);
    assert_eq!(update_resp["result"]["isError"], false);
    let update_parsed: Value = serde_json::from_str(
        update_resp["result"]["content"][0]["text"].as_str().unwrap()
    ).unwrap();
    let update_used_in = update_parsed["used_in"].as_array().expect("used_in array");
    assert_eq!(update_used_in.len(), 1);
    assert_eq!(update_used_in[0]["id"], "runbook/deploy");

    client.close();
}

#[test]
fn test_mcp_runbook_create_and_update() {
    let (_temp, root) = fixture_git_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &["--allow-write"]));
    client.init();

    // 1. Create valid runbook
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "type": "runbook",
                "title": "Restart Services",
                "tags": ["ops"],
                "steps": [
                    {
                        "id": "stop",
                        "title": "Stop service",
                        "entry": "cmd/deploy"
                    },
                    {
                        "id": "wait",
                        "title": "Wait 30s",
                        "do": "Wait for gracefully shutdown"
                    }
                ]
            }
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 6);
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value = serde_json::from_str(
        resp["result"]["content"][0]["text"].as_str().unwrap()
    ).unwrap();
    assert_eq!(parsed["id"], "runbook/restart-services");
    assert!(root.join("runbook/restart-services.md").exists());

    // Verify git trailer Kb-Source: mcp
    let git_out = Command::new("git")
        .args(["log", "-1", "--pretty=%B"])
        .current_dir(&root)
        .output()
        .unwrap();
    let commit_msg = String::from_utf8(git_out.stdout).unwrap();
    assert!(commit_msg.contains("Kb-Source: mcp"));

    // Verify marked unreviewed
    assert!(kb_core::mcp::ReviewStateTracker::is_unreviewed(
        &root,
        "runbook/restart-services"
    ));

    // 2. Create invalid runbook referencing non-existent entry -> fails
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "type": "runbook",
                "title": "Broken Runbook",
                "steps": [
                    {
                        "id": "broken",
                        "title": "Non-existent",
                        "entry": "missing/entry"
                    }
                ]
            }
        }
    }));
    let err_resp = client.recv();
    assert_eq!(err_resp["id"], 7);
    assert_eq!(err_resp["result"]["isError"], true);
    assert!(err_resp["result"]["content"][0]["text"].as_str().unwrap().contains("KB082"));

    // 3. Update runbook steps
    let version = parsed["version"].as_str().unwrap().to_string();
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {
            "name": "update",
            "arguments": {
                "id": "runbook/restart-services",
                "version": version,
                "changes": {
                    "steps": [
                        {
                            "id": "check",
                            "title": "Check errors first",
                            "entry": "sumo/5xx"
                        }
                    ]
                }
            }
        }
    }));
    let update_resp = client.recv();
    assert_eq!(update_resp["id"], 8);
    assert_eq!(update_resp["result"]["isError"], false);

    let content = fs::read_to_string(root.join("runbook/restart-services.md")).unwrap();
    assert!(content.contains("entry: sumo/5xx"));
    assert!(!content.contains("entry: cmd/deploy"));

    client.close();
}
