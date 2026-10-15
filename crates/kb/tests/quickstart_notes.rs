//! The scenarios of `specs/014-markdown-notes/quickstart.md`, end to end through the `kb` binary.

mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use support::pty::{Session, Spec, kb_home};
use support::{kb_in, search_kb};

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", text(&out.stderr));
    text(&out.stdout)
}

/// Scenarios 1 and 2: author a note in the editor, find it by a word from its body, read it.
#[test]
fn scenario_1_and_2_author_search_and_show() {
    let (t, home, root) = kb_home("search");
    let content = t.path().join("typed");
    fs::write(
        &content,
        "---\nkb_format: 1\ntype: note\ntitle: Redis Role Cache TTL\ntags: [redis, architecture]\n---\n\n# Redis Role Cache TTL\n\nThe authorizer caches user permissions in Redis for 300 seconds to protect the PostgreSQL primary database.\n\n## Invalidation Rules\n\n- Roles revoked: immediate DEL\n- Session timeout: natural eviction\n",
    )
    .unwrap();
    let editor = t.path().join("ed");
    fs::write(
        &editor,
        format!("#!/bin/sh\ncat '{}' > \"$1\"\n", content.display()),
    )
    .unwrap();
    fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(t.path().join("tmp")).unwrap();
    let spec = Spec::new(
        &[
            "note",
            "Redis Role Cache TTL",
            "--tags",
            "redis,architecture",
        ],
        &home,
        &root,
    )
    .env("TMPDIR", t.path().join("tmp").to_str().unwrap())
    .env("KB_EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(root.join("note/redis-role-cache-ttl.md").is_file());

    let out = kb_in(&root)
        .args(["search", "postgresql eviction", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        doc["results"][0]["id"], "note/redis-role-cache-ttl",
        "{doc}"
    );
    let snippet = doc["results"][0]["snippet"]["text"].as_str().unwrap();
    assert!(
        snippet.contains("PostgreSQL") || snippet.contains("eviction"),
        "{snippet}"
    );

    let shown = text(
        &kb_in(&root)
            .args(["show", "note/redis-role-cache-ttl"])
            .output()
            .unwrap()
            .stdout,
    );
    assert!(
        shown.contains("━━━") && shown.contains("• Roles revoked")
            || shown.contains("Roles revoked"),
        "{shown}"
    );
    assert!(!shown.contains("kb_format"), "{shown}");
    let raw = text(
        &kb_in(&root)
            .args(["show", "--raw", "note/redis-role-cache-ttl"])
            .output()
            .unwrap()
            .stdout,
    );
    assert!(
        raw.starts_with("# Redis Role Cache TTL\n") && !raw.contains('\u{1b}'),
        "{raw}"
    );
}

/// Scenario 3: the note of the guide, written literally, and the same with real escape bytes.
#[test]
fn scenario_3_a_hostile_note_is_inert() {
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join("note")).unwrap();
    for (name, esc) in [
        ("security-test", "\\x1b[2J\\x1b[H\\x1b]0;Pwned\\x07"),
        ("security-real", "\x1b[2J\x1b[H\x1b]0;Pwned\x07"),
    ] {
        fs::write(
            root.join(format!("note/{name}.md")),
            format!(
                "---\nkb_format: 1\ntype: note\ntitle: \"Security Test\"\n---\n\n# Malicious Note\n\nHere is a dangerous terminal sequence: {esc}\n\nHere is raw HTML: <script>alert(\"evil\")</script>\n\nHere is a bad link: [Click me](javascript:alert(1))\n"
            ),
        )
        .unwrap();
        let shown = text(
            &kb_in(&root)
                .args(["show", &format!("note/{name}")])
                .output()
                .unwrap()
                .stdout,
        );
        assert!(
            shown.chars().all(|c| c == '\n' || !c.is_control()),
            "{shown:?}"
        );
        assert!(
            shown.contains("<script>alert(\"evil\")</script>"),
            "{shown}"
        );
        assert!(
            shown.contains("Click me") && !shown.contains("javascript"),
            "{shown}"
        );
        assert!(!shown.contains("]8;"), "no hyperlink for javascript:");
    }
    let real = text(
        &kb_in(&root)
            .args(["show", "note/security-real"])
            .output()
            .unwrap()
            .stdout,
    );
    assert!(real.contains('␛'), "the escape codes are visible: {real}");
}

/// Scenario 4: links render, and `kb mv` updates them in one commit.
#[test]
fn scenario_4_entry_links_and_atomic_refactoring() {
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join("note")).unwrap();
    fs::write(
        root.join("note/incident-playbook.md"),
        "---\nkb_format: 1\ntype: note\ntitle: Incident Playbook\n---\n\n# Incident Playbook\n\n1. Check operational telemetry in [[sumo/5xx-by-authorizer]].\n2. Run database health check in [[cmd/kubectl-pods]].\n3. Non-existent link: [[cw/missing-log-group]].\n",
    )
    .unwrap();
    let shown = text(
        &kb_in(&root)
            .args(["show", "note/incident-playbook"])
            .output()
            .unwrap()
            .stdout,
    );
    assert!(shown.contains("[sumo] 5xx by authorizer"), "{shown}");
    assert!(shown.contains("[cmd] "), "{shown}");
    assert!(shown.contains("[?] cw/missing-log-group"), "{shown}");

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let out = kb_in(&root)
        .args(["mv", "sumo/5xx-by-authorizer", "sumo/5xx-authorizer-v2"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let playbook = fs::read_to_string(root.join("note/incident-playbook.md")).unwrap();
    assert!(
        playbook.contains("[[sumo/5xx-authorizer-v2]]")
            && !playbook.contains("[[sumo/5xx-by-authorizer]]"),
        "{playbook}"
    );
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]).trim(), "2");
    let files = git(&root, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        files.contains("note/incident-playbook.md") && files.contains("sumo/5xx-authorizer-v2.md"),
        "{files}"
    );
}

/// Scenario 5: the agent never sees a private note.
#[test]
fn scenario_5_private_notes_are_hidden_from_agents() {
    let (_d, root) = search_kb();
    let made = kb_in(&root)
        .args([
            "note",
            "Internal Credentials Runbook",
            "--tags",
            "private,security",
            "--yes",
        ])
        .write_stdin("# Internal Credentials Runbook\n\nRotate the vault unseal keys quarterly.\n")
        .output()
        .unwrap();
    assert_eq!(made.status.code(), Some(0), "{}", text(&made.stderr));
    assert!(
        text(
            &kb_in(&root)
                .args(["search", "unseal", "--ids"])
                .output()
                .unwrap()
                .stdout
        )
        .contains("note/internal-credentials-runbook")
    );

    let mut child = Command::new(support::pty::KB)
        .arg("--root")
        .arg(&root)
        .arg("mcp")
        .env("HOME", support::NOWHERE)
        .env("XDG_CONFIG_HOME", format!("{}/config", support::NOWHERE))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |req: Value| -> Value {
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}));
    let resp = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "search", "arguments": {"query": "unseal"}}}));
    let found: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(found["results"].as_array().unwrap().is_empty(), "{found}");
    drop(stdin);
    child.wait().unwrap();
}
