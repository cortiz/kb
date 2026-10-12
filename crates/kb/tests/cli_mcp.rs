//! Integration tests for Model Context Protocol (MCP) server over stdio.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use serde_json::{json, Value};
use support::{pty, search_kb, NOWHERE};

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

fn kb_mcp_cmd(root: &Path) -> Command {
    kb_mcp_cmd_with_args(root, &[])
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

    fn close(mut self) {
        // Drop stdin to close pipe
        drop(self.child.stdin.take());
        let out = self.child.wait_with_output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "kb mcp exited with code: {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn test_mcp_initialize_and_ping() {
    let (_dir, root) = search_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd(&root));

    // 1. Initialize
    client.send(json!({
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
    let resp = client.recv();
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(resp["result"]["serverInfo"]["name"], "kb");
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    // 2. Initialized notification
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    // 3. Ping
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    }));
    let ping_resp = client.recv();
    assert_eq!(ping_resp["id"], 2);
    assert_eq!(ping_resp["result"], json!({}));

    client.close();
}

#[test]
fn test_mcp_tools_list_read_only_vs_write_allowed() {
    let (_dir, root) = search_kb();

    // 1. Read-only by default
    {
        let mut client = McpClient::spawn(kb_mcp_cmd(&root));
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize"
        }));
        let _ = client.recv();

        client.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }));
        let resp = client.recv();
        assert_eq!(resp["id"], 2);
        let tools = resp["result"]["tools"].as_array().unwrap();
        let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

        assert!(tool_names.contains(&"search"));
        assert!(tool_names.contains(&"get"));
        assert!(tool_names.contains(&"list_tags"));
        assert!(!tool_names.contains(&"create"));
        assert!(!tool_names.contains(&"update"));

        client.close();
    }

    // 2. Write allowed via --allow-write
    {
        let mut cmd = kb_mcp_cmd(&root);
        cmd.arg("--allow-write");
        let mut client = McpClient::spawn(cmd);

        client.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize"
        }));
        let _ = client.recv();

        client.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }));
        let resp = client.recv();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

        assert!(tool_names.contains(&"create"));
        assert!(tool_names.contains(&"update"));

        client.close();
    }
}

#[test]
fn test_mcp_search_authorizer() {
    let (_dir, root) = search_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd(&root));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize"
    }));
    let _ = client.recv();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "search",
            "arguments": {
                "query": "5xx auth"
            }
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);

    let content_json = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(content_json).unwrap();
    let results = parsed["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["id"], "sumo/5xx-by-authorizer");
    assert_eq!(results[0]["type"], "sumo");
    assert!(results[0]["snippet"].as_str().unwrap().contains("authorizer"));

    // Check FR-010: human-readable form present in content[1]
    let human_summary = resp["result"]["content"][1]["text"].as_str().unwrap();
    assert!(human_summary.contains("Found"));
    assert!(human_summary.contains("sumo/5xx-by-authorizer"));

    client.close();
}

#[test]
fn test_mcp_get_authorizer_with_resolve() {
    let (_dir, root) = search_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd(&root));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize"
    }));
    let _ = client.recv();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/5xx-by-authorizer",
                "resolve": true
            }
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);

    let content_json = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(content_json).unwrap();
    assert_eq!(parsed["id"], "sumo/5xx-by-authorizer");
    assert_eq!(parsed["type"], "sumo");
    assert!(!parsed["version"].as_str().unwrap().is_empty());
    assert!(parsed["resolved_payload"].as_str().unwrap().contains("500"));

    // Check action link generation
    assert!(parsed["links"]["console_url"].as_str().unwrap().contains("sumologic.com"));

    client.close();
}

#[test]
fn test_mcp_list_tags() {
    let (_dir, root) = search_kb();
    let mut client = McpClient::spawn(kb_mcp_cmd(&root));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize"
    }));
    let _ = client.recv();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "list_tags",
            "arguments": {}
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 4);
    assert_eq!(resp["result"]["isError"], false);

    let content_json = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(content_json).unwrap();
    let tags = parsed["tags"].as_array().unwrap();
    assert!(!tags.is_empty());
    assert!(tags.iter().any(|t| t["name"] == "auth"));

    client.close();
}

#[test]
fn test_mcp_secrets_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let sumo_dir = root.join("sumo");
    std::fs::create_dir_all(&sumo_dir).unwrap();
    let entry_file = sumo_dir.join("secret-query.md");
    std::fs::write(
        &entry_file,
        "---\nkb_format: 1\ntype: sumo\ntitle: Secret Query\ntags: [auth]\nvars:\n  env: { kind: choice, options: [dev, prod], default: dev }\n  token: { kind: secret, env: SUMO_TOKEN }\n---\n```\n_sourceCategory={{env}} token={{token}}\n```\n",
    ).unwrap();

    let mut client = McpClient::spawn(kb_mcp_cmd(root));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize"
    }));
    let _ = client.recv();

    // 1. Resolve: token must resolve to $SUMO_TOKEN reference, never plaintext
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/secret-query",
                "resolve": true
            }
        }
    }));
    let resp = client.recv();
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value = serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let payload = parsed["resolved_payload"].as_str().unwrap();
    assert!(payload.contains("$SUMO_TOKEN"));

    // 2. Agent passes secret in vars: MUST BE REJECTED without echoing value
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/secret-query",
                "resolve": true,
                "vars": {
                    "token": "super-secret-key-12345"
                }
            }
        }
    }));
    let err_resp = client.recv();
    assert_eq!(err_resp["id"], 3);
    assert_eq!(err_resp["result"]["isError"], true);
    let err_text = err_resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!err_text.contains("super-secret-key-12345"));
    assert!(err_text.contains("cannot supply value for secret variable 'token'"));

    // 3. Secret without env source: placeholder retained + warning finding
    let entry_file2 = sumo_dir.join("noenv-query.md");
    std::fs::write(
        &entry_file2,
        "---\nkb_format: 1\ntype: sumo\ntitle: No Env Secret\nvars:\n  api_key: { kind: secret }\n---\n```\nkey={{api_key}}\n```\n",
    ).unwrap();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/noenv-query",
                "resolve": true
            }
        }
    }));
    let resp4 = client.recv();
    assert_eq!(resp4["id"], 4);
    assert_eq!(resp4["result"]["isError"], false);
    let parsed4: Value = serde_json::from_str(resp4["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(parsed4["resolved_payload"].as_str().unwrap().contains("{{api_key}}"));
    let findings = parsed4["findings"].as_array().unwrap();
    assert!(findings.iter().any(|f| f["rule"] == "secret_without_env"));

    client.close();
}

#[test]
fn test_mcp_setup_output() {
    let (_dir, root) = search_kb();
    let out = Command::new(pty::KB)
        .arg("--root")
        .arg(&root)
        .arg("mcp")
        .arg("setup")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("claude mcp add kb -- kb mcp"));
    assert!(stdout.contains("--allow-write"));
    assert!(stdout.contains("mcpServers"));
}

#[test]
fn test_doctor_mcp_check() {
    let (_dir, root) = search_kb();
    let out = Command::new(pty::KB)
        .arg("--root")
        .arg(&root)
        .arg("doctor")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let checks = parsed["checks"].as_array().unwrap();
    let mcp_check = checks.iter().find(|c| c["id"] == "mcp").expect("mcp check in doctor output");
    assert_eq!(mcp_check["status"], "pass");
    assert_eq!(mcp_check["summary"], "stdio server responsive");
    let details: Vec<&str> = mcp_check["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(details.iter().any(|d| d.contains("protocol: 2024-11-05")));
    assert!(details.iter().any(|d| d.contains("prompts: enabled")));
}


#[test]
fn test_mcp_create_entry() {
    let (_dir, root) = search_kb();
    let _ = Command::new("git").args(["init"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.name", "Test Agent"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.email", "agent@example.com"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["add", "-A"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["commit", "-m", "initial commit"]).current_dir(&root).output().unwrap();

    // 1. Read-only server rejects create
    let mut ro_client = McpClient::spawn(kb_mcp_cmd(&root));
    ro_client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize"
    }));
    let _ = ro_client.recv();

    ro_client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "title": "Blocked Entry",
                "payload": "echo blocked"
            }
        }
    }));
    let ro_resp = ro_client.recv();
    assert_eq!(ro_resp["id"], 2);
    assert_eq!(ro_resp["result"]["isError"], true);
    assert!(ro_resp["result"]["content"][0]["text"].as_str().unwrap().contains("Write tools are disabled"));
    ro_client.close();

    // 2. Server with --allow-write
    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &["--allow-write"]));
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "claude-code"
            }
        }
    }));
    let _ = client.recv();

    // Dry run: does not create file
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "title": "Dry Run Cmd",
                "type": "cmd",
                "payload": "kubectl get nodes",
                "path": "cmd/dry-run-cmd",
                "dry_run": true
            }
        }
    }));
    let dry_resp = client.recv();
    assert_eq!(dry_resp["id"], 11);
    assert_eq!(dry_resp["result"]["isError"], false);
    let dry_parsed: Value = serde_json::from_str(dry_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(dry_parsed["dry_run_content"].is_string());
    assert!(!root.join("cmd/dry-run-cmd.md").exists());

    // Secret rejection
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "title": "Secret Cmd",
                "type": "cmd",
                "payload": "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE",
                "path": "cmd/secret-cmd"
            }
        }
    }));
    let sec_resp = client.recv();
    assert_eq!(sec_resp["id"], 12);
    assert_eq!(sec_resp["result"]["isError"], true);
    let sec_err = sec_resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(sec_err.contains("Rule 60") || sec_err.contains("secret"));
    assert!(!sec_err.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(!root.join("cmd/secret-cmd.md").exists());

    // Successful create
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "tools/call",
        "params": {
            "name": "create",
            "arguments": {
                "title": "Tails Auth Logs",
                "type": "cmd",
                "payload": "kubectl logs -f -l app=authorizer",
                "path": "cmd/tail-auth"
            }
        }
    }));
    let create_resp = client.recv();
    assert_eq!(create_resp["id"], 13);
    assert_eq!(create_resp["result"]["isError"], false);
    let create_parsed: Value = serde_json::from_str(create_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(create_parsed["id"], "cmd/tail-auth");
    assert!(root.join("cmd/tail-auth.md").exists());

    // Verify git auto-commit with agent trailers
    let git_out = Command::new("git").args(["log", "-1", "--pretty=%B"]).current_dir(&root).output().unwrap();
    let commit_msg = String::from_utf8(git_out.stdout).unwrap();
    assert!(commit_msg.contains("Kb-Source: mcp"));
    assert!(commit_msg.contains("Kb-Client: claude-code"));

    // Verify marked unreviewed
    assert!(kb_core::mcp::ReviewStateTracker::is_unreviewed(&root, "cmd/tail-auth"));

    client.close();
}

#[test]
fn test_mcp_update_entry() {
    let (_dir, root) = search_kb();
    let _ = Command::new("git").args(["init"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.name", "Test Agent"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["config", "user.email", "agent@example.com"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["add", "-A"]).current_dir(&root).output().unwrap();
    let _ = Command::new("git").args(["commit", "-m", "initial commit"]).current_dir(&root).output().unwrap();

    let mut client = McpClient::spawn(kb_mcp_cmd_with_args(&root, &["--allow-write"]));
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "claude-code"
            }
        }
    }));
    let _ = client.recv();

    // 1. Get current version of sumo/5xx-by-authorizer
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get",
            "arguments": {
                "id": "sumo/5xx-by-authorizer"
            }
        }
    }));
    let get_resp = client.recv();
    assert_eq!(get_resp["id"], 2);
    let get_parsed: Value = serde_json::from_str(get_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let current_version = get_parsed["version"].as_str().unwrap().to_string();

    // 2. Update with wrong version is rejected
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "update",
            "arguments": {
                "id": "sumo/5xx-by-authorizer",
                "version": "bad-hash-000000000000",
                "changes": {
                    "add_tags": ["incident-2026"]
                }
            }
        }
    }));
    let bad_ver_resp = client.recv();
    assert_eq!(bad_ver_resp["id"], 3);
    assert_eq!(bad_ver_resp["result"]["isError"], true);
    assert!(bad_ver_resp["result"]["content"][0]["text"].as_str().unwrap().contains("version mismatch"));

    // 3. Update with dry_run
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "update",
            "arguments": {
                "id": "sumo/5xx-by-authorizer",
                "version": current_version,
                "dry_run": true,
                "changes": {
                    "add_tags": ["incident-2026"]
                }
            }
        }
    }));
    let dry_resp = client.recv();
    assert_eq!(dry_resp["id"], 4);
    assert_eq!(dry_resp["result"]["isError"], false);
    let dry_parsed: Value = serde_json::from_str(dry_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let diff = dry_parsed["dry_run_diff"].as_str().unwrap();
    assert!(diff.contains("incident-2026"));
    let disk_content = std::fs::read_to_string(root.join("sumo/5xx-by-authorizer.md")).unwrap();
    assert!(!disk_content.contains("incident-2026"));

    // 4. Update with actual save
    client.send(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "update",
            "arguments": {
                "id": "sumo/5xx-by-authorizer",
                "version": current_version,
                "changes": {
                    "add_tags": ["incident-2026"]
                }
            }
        }
    }));
    let update_resp = client.recv();
    assert_eq!(update_resp["id"], 5);
    assert_eq!(update_resp["result"]["isError"], false);
    let updated_disk = std::fs::read_to_string(root.join("sumo/5xx-by-authorizer.md")).unwrap();
    assert!(updated_disk.contains("incident-2026"));

    // Verify git auto-commit with agent trailers
    let git_out = Command::new("git").args(["log", "-1", "--pretty=%B"]).current_dir(&root).output().unwrap();
    let commit_msg = String::from_utf8(git_out.stdout).unwrap();
    assert!(commit_msg.contains("Kb-Source: mcp"));
    assert!(commit_msg.contains("Kb-Client: claude-code"));

    // Verify marked unreviewed
    assert!(kb_core::mcp::ReviewStateTracker::is_unreviewed(&root, "sumo/5xx-by-authorizer"));

    client.close();
}

#[test]
fn test_mcp_prompts_list_and_get() {
    let (_dir, root) = search_kb();
    let prompt_dir = root.join("prompt");
    std::fs::create_dir_all(&prompt_dir).unwrap();

    // 1. Valid prompt with variables
    let custom_prompt = prompt_dir.join("incident-summary.md");
    std::fs::write(
        &custom_prompt,
        "---\nkb_format: 1\ntype: prompt\ntitle: Custom Incident Summary\ntags: [incident]\nvars:\n  target: { kind: text, description: \"Target service\" }\n  env: { kind: text, default: \"dev\" }\n---\n\n```\nAnalyze 5xx on {{target}} in {{env}}.\n```\n",
    ).unwrap();

    // 2. Secret prompt (must be excluded)
    let secret_prompt = prompt_dir.join("secret-prompt.md");
    std::fs::write(
        &secret_prompt,
        "---\nkb_format: 1\ntype: prompt\ntitle: Leaky prompt\nvars:\n  token: { kind: secret, env: TOKEN }\n---\n\n```\nLeaked: {{token}}\n```\n",
    ).unwrap();

    // 3. Test with prompts enabled (default)
    {
        let mut client = McpClient::spawn(kb_mcp_cmd(&root));

        // Initialize -> capabilities.prompts present
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "claude-code"
                }
            }
        }));
        let init_resp = client.recv();
        assert_eq!(init_resp["id"], 1);
        assert!(init_resp["result"]["capabilities"]["prompts"].is_object());

        // prompts/list
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list"
        }));
        let list_resp = client.recv();
        assert_eq!(list_resp["id"], 2);
        let prompts = list_resp["result"]["prompts"].as_array().unwrap();
        let prompt_names: Vec<&str> = prompts.iter().map(|p| p["name"].as_str().unwrap()).collect();

        // Custom prompt is present
        assert!(prompt_names.contains(&"prompt-incident-summary"));
        // Secret prompt is excluded
        assert!(!prompt_names.contains(&"prompt-secret-prompt"));

        let inc_prompt = prompts.iter().find(|p| p["name"] == "prompt-incident-summary").unwrap();
        assert_eq!(inc_prompt["description"], "Custom Incident Summary");
        let args = inc_prompt["arguments"].as_array().unwrap();
        let target_arg = args.iter().find(|a| a["name"] == "target").unwrap();
        assert_eq!(target_arg["required"], true);
        assert_eq!(target_arg["description"], "Target service");
        let env_arg = args.iter().find(|a| a["name"] == "env").unwrap();
        assert_eq!(env_arg["required"], false);

        // prompts/get with args
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/get",
            "params": {
                "name": "prompt-incident-summary",
                "arguments": {
                    "target": "authorizer",
                    "env": "prod"
                }
            }
        }));
        let get_resp = client.recv();
        assert_eq!(get_resp["id"], 3);
        let msg = &get_resp["result"]["messages"][0];
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"]["text"].as_str().unwrap().trim(), "Analyze 5xx on authorizer in prod.");

        // prompts/get with defaults
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "prompts/get",
            "params": {
                "name": "prompt-incident-summary",
                "arguments": {
                    "target": "auth-service"
                }
            }
        }));
        let get_resp2 = client.recv();
        assert_eq!(get_resp2["id"], 4);
        let msg2 = &get_resp2["result"]["messages"][0];
        assert_eq!(msg2["content"]["text"].as_str().unwrap().trim(), "Analyze 5xx on auth-service in dev.");

        // prompts/get for excluded or nonexistent prompt -> error
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "prompts/get",
            "params": {
                "name": "prompt-secret-prompt"
            }
        }));
        let err_resp = client.recv();
        assert_eq!(err_resp["id"], 5);
        assert!(err_resp["error"].is_object());

        client.close();
    }

    // 4. Test with prompts disabled via KB_MCP_PROMPTS=false
    {
        let mut cmd = kb_mcp_cmd(&root);
        cmd.env("KB_MCP_PROMPTS", "false");
        let mut client = McpClient::spawn(cmd);

        client.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize"
        }));
        let init_resp = client.recv();
        assert!(init_resp["result"]["capabilities"].get("prompts").is_none());

        // prompts/list should return error
        client.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list"
        }));
        let list_resp = client.recv();
        assert_eq!(list_resp["error"]["code"], -32601);

        client.close();
    }
}

