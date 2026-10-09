//! The parts of the platform layer that are the same on Linux and macOS: the controlling
//! terminal and launching an editor.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

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

/// The command that opens `path` in `visual`, else `editor`, else `vi`, with stdin, stdout and
/// stderr on the terminal `tty` (stdout may be a pipe). The shell splits the editor command, so
/// `code -w` works; the path is passed as an argument and never re-parsed.
pub fn editor_command(
    visual: Option<&OsStr>,
    editor: Option<&OsStr>,
    path: &Path,
    tty: &File,
) -> io::Result<Command> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(r#"exec ${VISUAL:-${EDITOR:-vi}} "$1""#)
        .arg("kb-editor")
        .arg(path);
    match visual {
        Some(v) => cmd.env("VISUAL", v),
        None => cmd.env_remove("VISUAL"),
    };
    match editor {
        Some(e) => cmd.env("EDITOR", e),
        None => cmd.env_remove("EDITOR"),
    };
    cmd.stdin(Stdio::from(tty.try_clone()?))
        .stdout(Stdio::from(tty.try_clone()?))
        .stderr(Stdio::from(tty.try_clone()?));
    Ok(cmd)
}

/// Runs the editor until it exits. When `interrupt` turns true (a signal arrived) the editor is
/// killed and `None` is returned.
pub fn run_editor(
    path: &Path,
    tty: &File,
    interrupt: &dyn Fn() -> bool,
) -> io::Result<Option<ExitStatus>> {
    let visual: Option<OsString> = std::env::var_os("VISUAL");
    let editor: Option<OsString> = std::env::var_os("EDITOR");
    let child = editor_command(visual.as_deref(), editor.as_deref(), path, tty)?.spawn()?;
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
    fn run(
        visual: Option<&str>,
        editor: Option<&str>,
        path: &Path,
        dir: &Path,
    ) -> (ExitStatus, String) {
        let sink = dir.join("tty-sink");
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&sink)
            .unwrap();
        let mut cmd =
            editor_command(visual.map(OsStr::new), editor.map(OsStr::new), path, &tty).unwrap();
        cmd.env("PATH", format!("{}:/usr/bin:/bin", dir.display()));
        let status = cmd.status().unwrap();
        (status, std::fs::read_to_string(&sink).unwrap_or_default())
    }

    #[test]
    fn visual_beats_editor_beats_vi() {
        let d = tempfile::tempdir().unwrap();
        let v = script(d.path(), "myvisual", r#"echo "visual:$1""#);
        let e = script(d.path(), "myeditor", r#"echo "editor:$1""#);
        script(d.path(), "vi", r#"echo "vi:$1""#);
        let file = d.path().join("entry.md");
        let (st, out) = run(
            Some(v.to_str().unwrap()),
            Some(e.to_str().unwrap()),
            &file,
            d.path(),
        );
        assert!(st.success());
        assert_eq!(out.trim(), format!("visual:{}", file.display()));
        let (_, out) = run(None, Some(e.to_str().unwrap()), &file, d.path());
        assert_eq!(out.trim(), format!("editor:{}", file.display()));
        let (_, out) = run(Some(""), Some(""), &file, d.path());
        assert_eq!(out.trim(), format!("vi:{}", file.display()));
        let (_, out) = run(None, None, &file, d.path());
        assert_eq!(out.trim(), format!("vi:{}", file.display()));
    }

    #[test]
    fn editors_with_arguments_and_paths_with_spaces_work() {
        let d = tempfile::tempdir().unwrap();
        let ed = script(d.path(), "ed", r#"echo "$#:$1:$2:$3""#);
        let file = d.path().join("my entry.md");
        let (st, out) = run(
            None,
            Some(&format!("{} -w --flag", ed.display())),
            &file,
            d.path(),
        );
        assert!(st.success());
        assert_eq!(out.trim(), format!("3:-w:--flag:{}", file.display()));
    }

    #[test]
    fn the_exit_status_is_returned() {
        let d = tempfile::tempdir().unwrap();
        let ed = script(d.path(), "fail", "exit 3");
        let (st, _) = run(
            None,
            Some(ed.to_str().unwrap()),
            &d.path().join("x.md"),
            d.path(),
        );
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
}
