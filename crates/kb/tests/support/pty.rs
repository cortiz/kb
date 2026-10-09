//! Drives the real `kb` under a pseudo-terminal and reads its screen with a terminal emulator.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const KB: &str = env!("CARGO_BIN_EXE_kb");
pub const TIMEOUT: Duration = Duration::from_secs(30);

pub const ENTER: &[u8] = b"\r";
pub const ESC: &[u8] = b"\x1b";
pub const CTRL_C: &[u8] = b"\x03";
pub const TAB: &[u8] = b"\t";
pub const BACKTAB: &[u8] = b"\x1b[Z";
pub const DOWN: &[u8] = b"\x1b[B";
pub const UP: &[u8] = b"\x1b[A";
pub const SHIFT_DOWN: &[u8] = b"\x1b[1;2B";
pub const SHIFT_UP: &[u8] = b"\x1b[1;2A";
pub const BACKSPACE: &[u8] = b"\x7f";
pub const F1: &[u8] = b"\x1bOP";
pub const ALT_P: &[u8] = b"\x1bp";
pub const CTRL_E: &[u8] = b"\x05";
pub const CTRL_Y: &[u8] = b"\x19";
pub const CTRL_L: &[u8] = b"\x0c";
pub const CTRL_U: &[u8] = b"\x15";

pub struct Session {
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<Vec<u8>>>,
}

pub struct Spec {
    pub args: Vec<String>,
    pub home: PathBuf,
    pub root: PathBuf,
    /// stdout goes here (a file) instead of the terminal.
    pub out: Option<PathBuf>,
    /// Extra shell after kb exits (for example `stty -a`); kb is then not exec'd.
    pub after: Option<String>,
    /// stdout is a pipe into `cat > out` instead of a file.
    pub pipe: bool,
    pub env: Vec<(String, String)>,
    pub size: (u16, u16),
    /// kb's stdin is a pipe carrying this text instead of the terminal.
    pub stdin_text: Option<String>,
}

impl Spec {
    pub fn new(args: &[&str], home: &Path, root: &Path) -> Spec {
        Spec {
            args: args.iter().map(|a| (*a).to_string()).collect(),
            home: home.to_path_buf(),
            root: root.to_path_buf(),
            out: None,
            after: None,
            pipe: false,
            env: Vec::new(),
            size: (120, 40),
            stdin_text: None,
        }
    }

    pub fn env(mut self, key: &str, value: &str) -> Spec {
        self.env.push((key.to_string(), value.to_string()));
        self
    }
}

impl Session {
    pub fn spawn(spec: &Spec) -> Session {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: spec.size.1,
                cols: spec.size.0,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("a pty");
        let out = spec
            .out
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let redirect = if spec.out.is_none() {
            ""
        } else if spec.pipe {
            "| cat >\"$OUT\""
        } else {
            ">\"$OUT\""
        };
        let script = match &spec.after {
            Some(after) => format!("\"$KB\" \"$@\" {redirect}; echo \"exit=$?\"; {after}"),
            None => format!("exec \"$KB\" \"$@\" {redirect}"),
        };
        let script = match &spec.stdin_text {
            Some(_) => format!(
                "printf '%s\\n' \"$STDIN_TEXT\" | {}",
                script.replacen("exec ", "", 1)
            ),
            None => script,
        };
        // `exec` and a pipe do not mix; the pipe variant keeps the shell as the parent.
        let script = if spec.pipe && spec.after.is_none() {
            format!("\"$KB\" \"$@\" {redirect}")
        } else {
            script
        };
        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg(script);
        cmd.arg("sh");
        for a in &spec.args {
            cmd.arg(a);
        }
        cmd.env_clear();
        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.env("HOME", &spec.home);
        cmd.env("KB_ROOT", &spec.root);
        cmd.env("TERM", "xterm-256color");
        cmd.env("KB", KB);
        cmd.env("OUT", out);
        cmd.env("STDIN_TEXT", spec.stdin_text.clone().unwrap_or_default());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).expect("spawn");
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().expect("reader");
        let writer = pair.master.take_writer().expect("writer");
        let parser = Arc::new(Mutex::new(vt100::Parser::new(spec.size.1, spec.size.0, 0)));
        let raw = Arc::new(Mutex::new(Vec::new()));
        {
            let (parser, raw) = (Arc::clone(&parser), Arc::clone(&raw));
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    raw.lock().unwrap().extend_from_slice(&buf[..n]);
                    parser.lock().unwrap().process(&buf[..n]);
                }
            });
        }
        Session {
            child,
            writer,
            master: pair.master,
            parser,
            raw,
        }
    }

    pub fn screen(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    pub fn alternate_screen(&self) -> bool {
        self.parser.lock().unwrap().screen().alternate_screen()
    }

    pub fn cursor_hidden(&self) -> bool {
        self.parser.lock().unwrap().screen().hide_cursor()
    }

    /// The screen's rows, for tests that look at individual lines.
    pub fn rows(&self) -> Vec<String> {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        let (rows, cols) = s.size();
        (0..rows)
            .map(|r| s.contents_between(r, 0, r, cols))
            .collect()
    }

    /// Every cell has the default colors (the `NO_COLOR` check).
    pub fn has_no_colors(&self) -> bool {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        let (rows, cols) = s.size();
        (0..rows).all(|r| {
            (0..cols).all(|c| {
                s.cell(r, c).is_none_or(|cell| {
                    cell.fgcolor() == vt100::Color::Default
                        && cell.bgcolor() == vt100::Color::Default
                })
            })
        })
    }

    /// The background of one cell.
    pub fn bg_at(&self, row: u16, col: u16) -> vt100::Color {
        let p = self.parser.lock().unwrap();
        p.screen()
            .cell(row, col)
            .map_or(vt100::Color::Default, |c| c.bgcolor())
    }

    pub fn raw_output(&self) -> String {
        String::from_utf8_lossy(&self.raw.lock().unwrap()).into_owned()
    }

    pub fn wait_for(&self, text: &str) {
        self.wait_until(&format!("screen to contain {text:?}"), |s| s.contains(text));
    }

    pub fn wait_until(&self, what: &str, f: impl Fn(&str) -> bool) {
        let start = Instant::now();
        loop {
            let s = self.screen();
            if f(&s) {
                return;
            }
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out waiting for {what}; screen:\n{s}\n--- raw tail ---\n{:?}",
                self.raw_output()
                    .chars()
                    .rev()
                    .take(300)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("write to the pty");
        self.writer.flush().expect("flush");
    }

    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let mut b = [0u8; 4];
            self.send(c.encode_utf8(&mut b).as_bytes());
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize");
        self.parser.lock().unwrap().set_size(rows, cols);
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn signal(&self, name: &str) {
        let pid = self.pid().expect("pid");
        let _ = std::process::Command::new("kill")
            .arg(format!("-{name}"))
            .arg(pid.to_string())
            .status();
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn wait_exit(&mut self) -> u32 {
        let start = Instant::now();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                std::thread::sleep(Duration::from_millis(50)); // let the reader drain
                return status.exit_code();
            }
            assert!(
                start.elapsed() < TIMEOUT,
                "the process did not exit; screen:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

/// A copy of `tests/fixtures/<name>` at `<tmp>/home/kb`, so the header shows `~/kb`.
pub fn kb_home(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = home.join("kb");
    super::copy_dir(&super::fixtures().join(name), &root);
    (tmp, home, root)
}
