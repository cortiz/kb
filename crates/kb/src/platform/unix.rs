//! The parts of the platform layer that are the same on Linux and macOS: the controlling
//! terminal and launching an editor.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::{
    CopyTool, Display, Editor, Environment, Opened, OpenerCommand, ToolError, pick_editor,
};

/// Whether the TUI can talk to a user: stdin is a terminal, or the controlling terminal opens.
pub fn terminal_available(stdin_is_tty: bool, can_open_tty: bool) -> bool {
    stdin_is_tty || can_open_tty
}

pub fn has_terminal() -> bool {
    terminal_available(io::stdin().is_terminal(), open_tty().is_ok())
}

/// The controlling terminal, read-write. The TUI draws here so stdout stays free for the pick.
pub fn open_tty() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open("/dev/tty")
}

/// The editor to run (FR-013): `configured` first, then `$VISUAL`, then `$EDITOR`, then `vi`.
pub fn resolve_editor(configured: Option<&str>) -> Editor {
    pick_editor(
        configured,
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
    )
}

/// The command that opens `path` in `editor`, with stdin, stdout and stderr on the terminal `tty`
/// (stdout may be a pipe). The shell splits the editor command, so `code -w` works; the path is
/// passed as an argument and never re-parsed.
pub fn editor_command(editor: &Editor, path: &Path, tty: &File) -> io::Result<Command> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(r#"exec $1 "$2""#)
        .arg("kb-editor")
        .arg(&editor.command)
        .arg(path);
    cmd.stdin(Stdio::from(tty.try_clone()?))
        .stdout(Stdio::from(tty.try_clone()?))
        .stderr(Stdio::from(tty.try_clone()?));
    Ok(cmd)
}

/// Runs the editor until it exits. When `interrupt` turns true (a signal arrived) the editor is
/// killed and `None` is returned.
pub fn run_editor(
    editor: &Editor,
    path: &Path,
    tty: &File,
    interrupt: &dyn Fn() -> bool,
) -> io::Result<Option<ExitStatus>> {
    let child = editor_command(editor, path, tty)?.spawn()?;
    wait_unless_interrupted(child, interrupt)
}

fn wait_unless_interrupted(
    mut child: std::process::Child,
    interrupt: &dyn Fn() -> bool,
) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if interrupt() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// ---- programs on PATH ----

/// The first executable file called `name` in the `path` list.
pub fn find_tool_in(path: &OsStr, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

pub fn find_tool(name: &str) -> Option<PathBuf> {
    find_tool_in(&std::env::var_os("PATH")?, name)
}

// ---- clipboard programs ----

/// Runs `cmd` with `text` on stdin and output discarded. Output must be discarded: programs like
/// `xclip` and `wl-copy` fork to keep serving the selection, and a forked child that inherits a
/// pipe keeps it open, so waiting for the output would hang until the selection is replaced. The
/// text is written from a helper thread so a program that stops reading cannot block us, and the
/// program is killed when `timeout` runs out.
pub fn run_tool(mut cmd: Command, text: &str, timeout: Duration) -> Result<(), ToolError> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ToolError::NotFound,
        _ => ToolError::Io(e.to_string()),
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let data = text.as_bytes().to_vec();
        // ends by itself: the write fails once the program exits or is killed
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(ToolError::Failed(status.code())),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(ToolError::Io(e.to_string())),
        }
    }
}

pub fn copy_command(tool: &CopyTool) -> Command {
    match tool {
        CopyTool::Native(t) => {
            let mut cmd = Command::new(t.name);
            cmd.args(t.args);
            cmd
        }
        CopyTool::Shell(line) => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(line);
            cmd
        }
    }
}

pub fn run_copy_tool(tool: &CopyTool, text: &str, timeout: Duration) -> Result<(), ToolError> {
    run_tool(copy_command(tool), text, timeout)
}

// ---- terminal ----

/// Writes `bytes` to the controlling terminal. Never touches stdout.
pub fn write_tty(bytes: &[u8]) -> io::Result<()> {
    let mut tty = OpenOptions::new().write(true).open("/dev/tty")?;
    tty.write_all(bytes)?;
    tty.flush()
}

// ---- tmux ----

/// Runs `cmd` and returns whether it succeeded and what it printed, or `None` if it could not
/// run or took longer than `timeout`.
fn capture(mut cmd: Command, timeout: Duration) -> Option<(bool, String)> {
    use std::io::Read;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().ok()? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some((status.success(), out))
}

/// tmux 3.3+ drops forwarded sequences unless `allow-passthrough` is `on` or `all`. Older tmux
/// has no such option (the command fails) and always forwarded them.
pub fn passthrough_from(command_succeeded: bool, output: &str) -> bool {
    !command_succeeded || output.trim() != "off"
}

pub fn tmux_allows_passthrough(env: &Environment) -> Option<bool> {
    if !env.tmux {
        return None;
    }
    let mut cmd = Command::new("tmux");
    cmd.args(["show-options", "-gv", "allow-passthrough"]);
    Some(match capture(cmd, Duration::from_secs(1)) {
        Some((ok, out)) => passthrough_from(ok, &out),
        None => true,
    })
}

// ---- opening ----

/// Over SSH with no display there is nothing to open a browser on.
pub fn nowhere_to_open(env: &Environment) -> bool {
    env.ssh && env.display == Display::None
}

pub fn open(target: &str, opener: &OpenerCommand, env: &Environment) -> io::Result<Opened> {
    if nowhere_to_open(env) {
        return Ok(Opened::Printed(target.to_string()));
    }
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(r#"exec $1 "$2""#)
        .arg("kb-open")
        .arg(&opener.command)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // ponytail: openers return at once; one that lingers is left running after 2 s
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match child.try_wait()? {
            Some(status) if !status.success() => {
                return Err(io::Error::other(format!(
                    "{} exited with status {}",
                    opener.command,
                    status.code().unwrap_or(-1)
                )));
            }
            Some(_) => break,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    Ok(Opened::Launched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_terminal_decision_is_a_pure_function() {
        assert!(terminal_available(true, false));
        assert!(terminal_available(false, true));
        assert!(terminal_available(true, true));
        assert!(!terminal_available(false, false));
    }

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// Runs the editor command with a stand-in for the terminal (a file) and `vi` shadowed.
    fn run(command: &str, path: &Path, dir: &Path) -> (ExitStatus, String) {
        let sink = dir.join("tty-sink");
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&sink)
            .unwrap();
        let editor = Editor {
            command: command.to_string(),
            source: super::super::EditorSource::Config,
        };
        let mut cmd = editor_command(&editor, path, &tty).unwrap();
        cmd.env("PATH", format!("{}:/usr/bin:/bin", dir.display()));
        let status = cmd.status().unwrap();
        (status, std::fs::read_to_string(&sink).unwrap_or_default())
    }

    #[test]
    fn the_resolved_editor_runs_with_the_file_as_its_argument() {
        let d = tempfile::tempdir().unwrap();
        let ed = script(d.path(), "myeditor", r#"echo "editor:$1""#);
        script(d.path(), "vi", r#"echo "vi:$1""#);
        let file = d.path().join("entry.md");
        let (st, out) = run(ed.to_str().unwrap(), &file, d.path());
        assert!(st.success());
        assert_eq!(out.trim(), format!("editor:{}", file.display()));
        // the fallback is vi, found on PATH
        let fallback = pick_editor(None, None, None);
        let (_, out) = run(&fallback.command, &file, d.path());
        assert_eq!(out.trim(), format!("vi:{}", file.display()));
    }

    #[test]
    fn editors_with_arguments_and_paths_with_spaces_work() {
        let d = tempfile::tempdir().unwrap();
        let ed = script(d.path(), "ed", r#"echo "$#:$1:$2:$3""#);
        let file = d.path().join("my entry.md");
        let (st, out) = run(&format!("{} -w --flag", ed.display()), &file, d.path());
        assert!(st.success());
        assert_eq!(out.trim(), format!("3:-w:--flag:{}", file.display()));
    }

    #[test]
    fn the_exit_status_is_returned() {
        let d = tempfile::tempdir().unwrap();
        let ed = script(d.path(), "fail", "exit 3");
        let (st, _) = run(ed.to_str().unwrap(), &d.path().join("x.md"), d.path());
        assert_eq!(st.code(), Some(3));
    }

    #[test]
    fn an_interrupt_kills_the_editor_and_returns_none() {
        let sleeper = || Command::new("sleep").arg("30").spawn().unwrap();
        let t = std::time::Instant::now();
        assert_eq!(wait_unless_interrupted(sleeper(), &|| true).unwrap(), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
        // without an interrupt the exit status comes back
        let quick = Command::new("sh").args(["-c", "exit 4"]).spawn().unwrap();
        let st = wait_unless_interrupted(quick, &|| false).unwrap().unwrap();
        assert_eq!(st.code(), Some(4));
    }

    // ---- clipboard programs, PATH, tmux, opening ----

    fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
        script(dir, name, body)
    }

    fn run_script(script_path: &Path, text: &str, ms: u64) -> Result<(), ToolError> {
        run_tool(Command::new(script_path), text, Duration::from_millis(ms))
    }

    #[test]
    fn a_program_that_succeeds_receives_the_text() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("got");
        let t = tool(d.path(), "ok", &format!("cat > {}", out.display()));
        assert_eq!(run_script(&t, "hello\nworld", 5_000), Ok(()));
        assert_eq!(std::fs::read_to_string(out).unwrap(), "hello\nworld");
    }

    #[test]
    fn a_failing_program_reports_its_status() {
        let d = tempfile::tempdir().unwrap();
        let t = tool(d.path(), "bad", "cat >/dev/null; exit 3");
        assert_eq!(run_script(&t, "x", 5_000), Err(ToolError::Failed(Some(3))));
    }

    #[test]
    fn a_missing_program_is_not_found() {
        assert_eq!(
            run_script(Path::new("/no/such/program"), "x", 1_000),
            Err(ToolError::NotFound)
        );
    }

    #[test]
    fn a_hung_program_is_killed_at_the_deadline() {
        let d = tempfile::tempdir().unwrap();
        let t = tool(d.path(), "hang", "sleep 30");
        let started = Instant::now();
        assert_eq!(run_script(&t, "x", 200), Err(ToolError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_program_that_never_reads_stdin_cannot_block_the_writer() {
        let d = tempfile::tempdir().unwrap();
        let t = tool(d.path(), "deaf", "sleep 30");
        let big = "x".repeat(1_000_000);
        let started = Instant::now();
        assert_eq!(run_script(&t, &big, 300), Err(ToolError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_program_that_forks_and_keeps_serving_returns_at_once() {
        // the xclip / wl-copy shape: read everything, fork a server, exit 0
        let d = tempfile::tempdir().unwrap();
        let t = tool(
            d.path(),
            "forker",
            "cat >/dev/null; (sleep 3 >/dev/null 2>&1 &) ; exit 0",
        );
        let started = Instant::now();
        assert_eq!(run_script(&t, "x", 5_000), Ok(()));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must not wait for the forked server"
        );
    }

    #[test]
    fn a_custom_command_runs_through_the_shell_with_arguments() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("got");
        let cmd = CopyTool::Shell(format!("tr a-z A-Z > '{}'", out.display()));
        assert_eq!(run_copy_tool(&cmd, "shout", Duration::from_secs(5)), Ok(()));
        assert_eq!(std::fs::read_to_string(out).unwrap(), "SHOUT");
        let missing = CopyTool::Shell("definitely-not-a-program-xyz".into());
        assert_eq!(
            run_copy_tool(&missing, "x", Duration::from_secs(5)),
            Err(ToolError::Failed(Some(127)))
        );
    }

    #[test]
    fn find_tool_needs_an_executable_file_on_the_path() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        let b = d.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let exe = script(&b, "pbcopy", "true");
        std::fs::write(a.join("pbcopy"), "not executable").unwrap();
        std::fs::create_dir(a.join("xclip")).unwrap();
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(find_tool_in(&path, "pbcopy"), Some(exe));
        assert_eq!(
            find_tool_in(&path, "xclip"),
            None,
            "a directory is not a tool"
        );
        assert_eq!(find_tool_in(&path, "wl-copy"), None);
    }

    #[test]
    fn tmux_passthrough_is_off_only_when_tmux_says_off() {
        assert!(passthrough_from(true, "on\n"));
        assert!(passthrough_from(true, "all\n"));
        assert!(!passthrough_from(true, "off\n"));
        assert!(
            passthrough_from(false, ""),
            "old tmux: no such option, always forwarded"
        );
    }

    fn env_with(vars: &[(&str, &str)], os: super::super::Os) -> Environment {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        Environment::detect(
            &move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
            os,
            "x86_64",
        )
    }

    #[test]
    fn outside_tmux_passthrough_is_not_asked() {
        let e = env_with(&[], super::super::Os::Linux);
        assert_eq!(tmux_allows_passthrough(&e), None);
    }

    #[test]
    fn opening_prints_the_url_over_ssh_without_a_display() {
        use super::super::Os;
        let opener = OpenerCommand {
            command: "xdg-open".into(),
        };
        let ssh = env_with(&[("SSH_CONNECTION", "a b c d")], Os::Linux);
        assert!(nowhere_to_open(&ssh));
        assert_eq!(
            open("https://example.com", &opener, &ssh).unwrap(),
            Opened::Printed("https://example.com".into())
        );
        let ssh_x11 = env_with(
            &[("SSH_CONNECTION", "a b c d"), ("DISPLAY", ":10")],
            Os::Linux,
        );
        assert!(!nowhere_to_open(&ssh_x11));
        assert!(
            !nowhere_to_open(&env_with(&[], Os::Linux)),
            "local, no display: still try"
        );
    }

    #[test]
    fn opening_runs_the_opener_with_the_target_as_one_argument() {
        use super::super::Os;
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("got");
        let opener = OpenerCommand {
            command: script(
                d.path(),
                "myopen",
                &format!("printf '%s' \"$1\" > {}", out.display()),
            )
            .display()
            .to_string(),
        };
        let local = env_with(&[], Os::MacOs);
        assert_eq!(
            open("https://example.com/a b?c=1&d=2", &opener, &local).unwrap(),
            Opened::Launched
        );
        assert_eq!(
            std::fs::read_to_string(out).unwrap(),
            "https://example.com/a b?c=1&d=2"
        );
        let failing = OpenerCommand {
            command: script(d.path(), "failopen", "exit 4").display().to_string(),
        };
        assert!(open("x", &failing, &local).is_err());
    }
}
