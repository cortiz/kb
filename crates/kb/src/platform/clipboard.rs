//! The clipboard: which programs apply where, the order methods are tried in, and `copy`.

use std::time::Duration;

use kb_core::config::{ClipboardChoice, Settings};

use super::osc52::{self, TooLarge, Wrap};
use super::{CopyTool, Display, Environment, NativeTool, Os, Platform, ToolError};

/// How long one copy program may take before the next method is tried (FR-009).
pub const COPY_TIMEOUT: Duration = Duration::from_secs(2);

pub const PBCOPY: NativeTool = NativeTool {
    name: "pbcopy",
    args: &[],
    install_hint: "pbcopy ships with macOS; check that /usr/bin is on PATH",
};
pub const WL_COPY: NativeTool = NativeTool {
    name: "wl-copy",
    args: &[],
    install_hint: "install wl-clipboard",
};
pub const XCLIP: NativeTool = NativeTool {
    name: "xclip",
    args: &["-selection", "clipboard", "-in"],
    install_hint: "install xclip or xsel",
};
pub const XSEL: NativeTool = NativeTool {
    name: "xsel",
    args: &["--clipboard", "--input"],
    install_hint: "install xclip or xsel",
};

pub const PBPASTE: NativeTool = NativeTool {
    name: "pbpaste",
    args: &[],
    install_hint: "pbpaste ships with macOS; check that /usr/bin is on PATH",
};
pub const WL_PASTE: NativeTool = NativeTool {
    name: "wl-paste",
    args: &[],
    install_hint: "install wl-clipboard",
};
pub const XCLIP_OUT: NativeTool = NativeTool {
    name: "xclip",
    args: &["-selection", "clipboard", "-out"],
    install_hint: "install xclip or xsel",
};
pub const XSEL_OUT: NativeTool = NativeTool {
    name: "xsel",
    args: &["--clipboard", "--output"],
    install_hint: "install xclip or xsel",
};

/// macOS: `pbcopy`.
pub fn mac_tools() -> Vec<NativeTool> {
    vec![PBCOPY]
}

/// Linux: Wayland's `wl-copy` first when `WAYLAND_DISPLAY` is set, then the X11 tools when
/// `DISPLAY` is set (so XWayland machines without `wl-copy` still work).
pub fn linux_tools(env: &Environment) -> Vec<NativeTool> {
    let mut tools = Vec::new();
    if env.display == Display::Wayland {
        tools.push(WL_COPY);
    }
    if env.x11 {
        tools.push(XCLIP);
        tools.push(XSEL);
    }
    tools
}

/// The programs that read the clipboard where `env` is, in the order they are tried: the mirror
/// of [`tools_for`].
pub fn read_tools_for(env: &Environment) -> Vec<NativeTool> {
    match env.os {
        Os::MacOs => vec![PBPASTE],
        Os::Linux => {
            let mut tools = Vec::new();
            if env.display == Display::Wayland {
                tools.push(WL_PASTE);
            }
            if env.x11 {
                tools.push(XCLIP_OUT);
                tools.push(XSEL_OUT);
            }
            tools
        }
    }
}

/// How long one read program may take before the next is tried (FR-001).
pub const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Why the clipboard could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// Over SSH the local clipboard is not the user's; OSC 52 can't be read back.
    OverSsh,
    NoTool {
        hint: String,
    },
    Failed(String),
    TimedOut,
    TooLarge,
}

impl Unavailable {
    /// One line saying why, for stderr or the form header.
    pub fn explain(&self) -> String {
        match self {
            Unavailable::OverSsh => "clipboard not readable over SSH".to_string(),
            Unavailable::NoTool { hint } => format!("no clipboard read tool found ({hint})"),
            Unavailable::Failed(why) => format!("clipboard not readable: {why}"),
            Unavailable::TimedOut => "clipboard read timed out".to_string(),
            Unavailable::TooLarge => "clipboard content is over 2 MiB".to_string(),
        }
    }
}

/// What reading the clipboard gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Text with `\n` line endings.
    Text(String),
    /// Nothing, or something that is not text (an image, a file).
    Empty,
    Unavailable(Unavailable),
}

/// Reads the clipboard text (FR-001 to FR-003, research R10). Never fails: what went wrong is an
/// `Unavailable` the caller turns into a fallback.
pub fn read(platform: &impl Platform) -> ReadOutcome {
    let env = platform.env();
    if env.ssh {
        return ReadOutcome::Unavailable(Unavailable::OverSsh);
    }
    let tools = read_tools_for(&env);
    let mut last_error = None;
    for (i, tool) in tools.iter().enumerate() {
        match platform.run_read_tool(tool, READ_TIMEOUT) {
            Ok(bytes) => return read_text(bytes),
            Err(ToolError::NotFound) => {}
            Err(ToolError::TooLarge) => return ReadOutcome::Unavailable(Unavailable::TooLarge),
            Err(ToolError::TimedOut) => last_error = Some(Unavailable::TimedOut),
            // `wl-paste` exits 1 when nothing is copied: from the last tool that is an empty
            // clipboard, not a failure
            Err(ToolError::Failed(_)) if i + 1 == tools.len() => return ReadOutcome::Empty,
            Err(e @ (ToolError::Failed(_) | ToolError::Io(_))) => {
                last_error = Some(Unavailable::Failed(e.to_string()));
            }
        }
    }
    ReadOutcome::Unavailable(last_error.unwrap_or_else(|| Unavailable::NoTool {
        hint: match tools.first() {
            Some(first) => first.install_hint.to_string(),
            None => "install wl-clipboard (Wayland) or xclip / xsel (X11); no display found".into(),
        },
    }))
}

/// Only text is used: bytes that are not UTF-8, or hold a NUL, are an image or a file.
fn read_text(bytes: Vec<u8>) -> ReadOutcome {
    let Ok(text) = String::from_utf8(bytes) else {
        return ReadOutcome::Empty;
    };
    if text.contains('\0') {
        return ReadOutcome::Empty;
    }
    let text = kb_core::author::normalize_clipboard(&text);
    if text.trim().is_empty() {
        ReadOutcome::Empty
    } else {
        ReadOutcome::Text(text)
    }
}

/// The programs that apply to `env`, in the order they are tried.
pub fn tools_for(env: &Environment) -> Vec<NativeTool> {
    match env.os {
        Os::MacOs => mac_tools(),
        Os::Linux => linux_tools(env),
    }
}

/// One way of getting text onto the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Native(NativeTool),
    /// `clipboard.command`.
    Custom(String),
    /// Ask the terminal (OSC 52).
    Osc52,
    /// Print the text; the caller does it.
    Stdout,
}

impl Method {
    /// What to call the method in messages.
    pub fn label(&self) -> String {
        match self {
            Method::Native(t) => t.name.to_string(),
            Method::Custom(c) => CopyTool::Shell(c.clone()).label(),
            Method::Osc52 => "terminal (OSC 52)".to_string(),
            Method::Stdout => "stdout".to_string(),
        }
    }
}

/// The clipboard settings, detached from where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSettings {
    pub choice: ClipboardChoice,
    pub command: Option<String>,
    pub osc52_max_bytes: usize,
    pub trim_trailing_newline: bool,
}

impl Default for ClipboardSettings {
    fn default() -> ClipboardSettings {
        ClipboardSettings {
            choice: ClipboardChoice::Auto,
            command: None,
            osc52_max_bytes: 74_000,
            trim_trailing_newline: true,
        }
    }
}

impl ClipboardSettings {
    pub fn from_settings(s: &Settings) -> ClipboardSettings {
        ClipboardSettings {
            choice: s.clipboard_method(),
            command: s.clipboard_command().map(str::to_string),
            osc52_max_bytes: s.osc52_max_bytes(),
            trim_trailing_newline: s.trim_trailing_newline(),
        }
    }
}

/// The methods to try, in order (FR-005, FR-006, research R4). A configured method comes first and
/// the others follow, so a forced method that fails still delivers. `Stdout` is always last.
pub fn chain(env: &Environment, s: &ClipboardSettings, tools: &[NativeTool]) -> Vec<Method> {
    // a custom command takes the place of the tools
    let native: Vec<Method> = match &s.command {
        Some(command) => vec![Method::Custom(command.clone())],
        None => tools.iter().copied().map(Method::Native).collect(),
    };
    let mut out = Vec::new();
    match s.choice {
        ClipboardChoice::Stdout => {}
        ClipboardChoice::Osc52 => {
            out.push(Method::Osc52);
            out.extend(native);
        }
        ClipboardChoice::Native => {
            out.extend(native);
            out.push(Method::Osc52);
        }
        ClipboardChoice::Auto => {
            // over SSH (even with X forwarding) or with no display a local tool would copy to the
            // wrong place or nowhere; the terminal reaches the machine the user sits at
            let terminal_first = s.command.is_none() && (env.ssh || env.display == Display::None);
            if !terminal_first {
                out.extend(native);
            }
            out.push(Method::Osc52);
        }
    }
    out.push(Method::Stdout);
    out
}

/// Where a copy is made. Inside the UI the screen belongs to the UI, so `Stdout` is not a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Cli,
    Ui,
}

/// A method that did not deliver, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub method: Method,
    pub reason: String,
    pub hint: Option<String>,
}

/// What `copy` did (data-model.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOutcome {
    /// What delivered the text. `None`: nothing did (see `refused` and `attempts`).
    pub used: Option<Method>,
    /// The text as copied (trailing newlines trimmed if configured).
    pub text: String,
    pub bytes: usize,
    pub lines: usize,
    /// The method reports success (a program exited 0). OSC 52 and stdout cannot.
    pub confirmed: bool,
    pub attempts: Vec<Attempt>,
    /// OSC 52 refused the payload for its size.
    pub refused: Option<TooLarge>,
    /// Extra advice, for example that tmux is not forwarding.
    pub notes: Vec<String>,
}

fn lines_text(n: usize) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
}

impl CopyOutcome {
    /// Whether the text was sent somewhere (decides whether a use is recorded).
    pub fn sent(&self) -> bool {
        self.used.is_some()
    }

    fn skip(&mut self, method: Method, reason: impl Into<String>, hint: Option<&str>) {
        self.attempts.push(Attempt {
            method,
            reason: reason.into(),
            hint: hint.map(str::to_string),
        });
    }

    /// The one line printed on stderr after a copy (FR-008): what happened and how.
    pub fn message(&self, id: &str) -> String {
        let n = lines_text(self.lines);
        match &self.used {
            Some(Method::Native(_) | Method::Custom(_)) => {
                let how = self.used.as_ref().map(Method::label).unwrap_or_default();
                format!("Copied {id} ({n}) via {how}")
            }
            Some(Method::Osc52) => format!("Sent {id} ({n}) via terminal (OSC 52)"),
            Some(Method::Stdout) if self.attempts.is_empty() => {
                format!("Printed {id} ({n}) to stdout")
            }
            Some(Method::Stdout) => {
                format!("Printed {id} ({n}) to stdout; no clipboard method worked")
            }
            None => match self.refused {
                Some(t) => format!(
                    "{id} is {} bytes, over clipboard.osc52_max_bytes ({}); nothing copied. \
                     Try: kb show --raw {id} | <clipboard tool>",
                    t.bytes, t.limit
                ),
                None => format!("nothing copied: {}", self.failure()),
            },
        }
    }

    /// The same for the key bar in the UI.
    pub fn message_short(&self) -> String {
        match &self.used {
            Some(Method::Native(_) | Method::Custom(_)) => {
                format!(
                    "Copied via {}",
                    self.used.as_ref().map(Method::label).unwrap_or_default()
                )
            }
            Some(Method::Osc52) => "Sent via terminal (OSC 52)".to_string(),
            Some(Method::Stdout) => "Printed to stdout".to_string(),
            None => match self.refused {
                Some(t) => format!(
                    "Too large for the terminal: {} B, limit {} B",
                    t.bytes, t.limit
                ),
                None => format!("Copy failed: {}", self.failure()),
            },
        }
    }

    fn failure(&self) -> String {
        match self.attempts.last() {
            Some(a) => format!("{}: {}", a.method.label(), a.reason),
            None => "no clipboard method is available here".to_string(),
        }
    }

    /// One `kb: …` line for every method that was skipped, plus any advice.
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .attempts
            .iter()
            .map(|a| {
                let hint = a
                    .hint
                    .as_ref()
                    .map(|h| format!(" ({h})"))
                    .unwrap_or_default();
                format!("kb: {} skipped: {}{hint}", a.method.label(), a.reason)
            })
            .collect();
        out.extend(self.notes.iter().map(|n| format!("kb: {n}")));
        out
    }
}

/// Copies `text` with the first method that works (FR-005 to FR-009). Never fails: `Stdout` ends
/// the chain in the CLI, and in the UI an outcome with `used: None` says why nothing worked.
pub fn copy(
    platform: &impl Platform,
    s: &ClipboardSettings,
    text: &str,
    place: Place,
) -> CopyOutcome {
    let env = platform.env();
    let text = if s.trim_trailing_newline {
        text.trim_end_matches(['\n', '\r'])
    } else {
        text
    };
    let mut o = CopyOutcome {
        used: None,
        text: text.to_string(),
        bytes: text.len(),
        lines: text.lines().count(),
        confirmed: false,
        attempts: Vec::new(),
        refused: None,
        notes: Vec::new(),
    };
    let mut methods = chain(&env, s, &platform.native_tools(&env));
    if place == Place::Ui {
        methods.retain(|m| *m != Method::Stdout);
    }
    for method in methods {
        let tool = match &method {
            Method::Native(t) => {
                if platform.find_tool(t.name).is_none() {
                    o.skip(method.clone(), "not found", Some(t.install_hint));
                    continue;
                }
                Some(CopyTool::Native(*t))
            }
            Method::Custom(command) => Some(CopyTool::Shell(command.clone())),
            Method::Osc52 | Method::Stdout => None,
        };
        if let Some(tool) = tool {
            match platform.run_copy_tool(&tool, &o.text, COPY_TIMEOUT) {
                Ok(()) => {
                    o.used = Some(method);
                    o.confirmed = true;
                    o.refused = None;
                    return o;
                }
                Err(e) => o.skip(method, e.to_string(), None),
            }
            continue;
        }
        if method == Method::Osc52 {
            match osc52::sequence(&o.text, Wrap::for_env(&env), s.osc52_max_bytes) {
                Err(too_large) => {
                    o.refused = Some(too_large);
                    o.skip(
                        method,
                        format!(
                            "payload is {} bytes, over clipboard.osc52_max_bytes ({})",
                            too_large.bytes, too_large.limit
                        ),
                        None,
                    );
                }
                Ok(sequence) => match platform.write_tty(sequence.as_bytes()) {
                    Ok(()) => {
                        o.used = Some(method);
                        o.refused = None;
                        if env.tmux && platform.tmux_allows_passthrough() == Some(false) {
                            o.notes.push(
                                "tmux is not forwarding terminal sequences; add \
                                 \"set -g allow-passthrough on\" to ~/.tmux.conf"
                                    .to_string(),
                            );
                        }
                        return o;
                    }
                    Err(e) => o.skip(method, format!("cannot write to the terminal: {e}"), None),
                },
            }
            continue;
        }
        // Stdout. When the only reason to get here is that the terminal refused the size, the
        // text is not dumped on the screen: the message points at `kb show --raw | <tool>`.
        if o.refused.is_none() {
            o.used = Some(Method::Stdout);
        }
        return o;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;

    fn linux(wayland: bool, x11: bool) -> Environment {
        let get = move |k: &str| match k {
            "WAYLAND_DISPLAY" if wayland => Some("wayland-0".to_string()),
            "DISPLAY" if x11 => Some(":0".to_string()),
            _ => None,
        };
        Environment::detect(&get, Os::Linux, "x86_64")
    }

    #[test]
    fn the_tool_arguments_are_exact() {
        assert_eq!(PBCOPY.args, &[] as &[&str]);
        assert_eq!(WL_COPY.args, &[] as &[&str]);
        assert_eq!(XCLIP.args, ["-selection", "clipboard", "-in"]);
        assert_eq!(XSEL.args, ["--clipboard", "--input"]);
        assert_eq!(mac_tools(), [PBCOPY]);
    }

    #[test]
    fn linux_tool_choice_follows_the_display() {
        assert_eq!(linux_tools(&linux(true, false)), [WL_COPY]);
        assert_eq!(linux_tools(&linux(false, true)), [XCLIP, XSEL]);
        assert_eq!(
            linux_tools(&linux(true, true)),
            [WL_COPY, XCLIP, XSEL],
            "Wayland tools first, X11 tools as the fallback"
        );
        assert!(linux_tools(&linux(false, false)).is_empty());
    }

    #[test]
    fn install_hints_name_a_package() {
        assert_eq!(WL_COPY.install_hint, "install wl-clipboard");
        assert_eq!(XCLIP.install_hint, "install xclip or xsel");
        assert_eq!(XSEL.install_hint, "install xclip or xsel");
    }

    // ---- the method chain ----

    use crate::platform::fake::{Call, FakePlatform, FakeTool, linux_env, mac_env};

    fn settings(choice: ClipboardChoice, command: Option<&str>) -> ClipboardSettings {
        ClipboardSettings {
            choice,
            command: command.map(str::to_string),
            ..ClipboardSettings::default()
        }
    }

    /// The chain as short names: `pbcopy`, `custom`, `osc52`, `stdout`.
    fn ids(env: &Environment, s: &ClipboardSettings) -> Vec<String> {
        chain(env, s, &tools_for(env))
            .into_iter()
            .map(|m| match m {
                Method::Native(t) => t.name.to_string(),
                Method::Custom(_) => "custom".to_string(),
                Method::Osc52 => "osc52".to_string(),
                Method::Stdout => "stdout".to_string(),
            })
            .collect()
    }

    #[test]
    fn the_method_chain_follows_the_spec_in_every_environment() {
        use ClipboardChoice::{Auto, Native, Osc52, Stdout};
        let ssh = ("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
        let rows: Vec<(&str, Environment, ClipboardSettings, Vec<&str>)> = vec![
            (
                "mac local",
                mac_env(&[]),
                settings(Auto, None),
                vec!["pbcopy", "osc52", "stdout"],
            ),
            (
                "mac ssh",
                mac_env(&[ssh]),
                settings(Auto, None),
                vec!["osc52", "stdout"],
            ),
            (
                "mac ssh tmux",
                mac_env(&[ssh, ("TMUX", "x")]),
                settings(Auto, None),
                vec!["osc52", "stdout"],
            ),
            (
                "mac ssh, native forced",
                mac_env(&[ssh]),
                settings(Native, None),
                vec!["pbcopy", "osc52", "stdout"],
            ),
            (
                "mac osc52 forced",
                mac_env(&[]),
                settings(Osc52, None),
                vec!["osc52", "pbcopy", "stdout"],
            ),
            (
                "mac stdout forced",
                mac_env(&[]),
                settings(Stdout, None),
                vec!["stdout"],
            ),
            (
                "wayland",
                linux_env(&[("WAYLAND_DISPLAY", "w")]),
                settings(Auto, None),
                vec!["wl-copy", "osc52", "stdout"],
            ),
            (
                "wayland + xwayland",
                linux_env(&[("WAYLAND_DISPLAY", "w"), ("DISPLAY", ":0")]),
                settings(Auto, None),
                vec!["wl-copy", "xclip", "xsel", "osc52", "stdout"],
            ),
            (
                "x11",
                linux_env(&[("DISPLAY", ":0")]),
                settings(Auto, None),
                vec!["xclip", "xsel", "osc52", "stdout"],
            ),
            (
                "x11 tmux",
                linux_env(&[("DISPLAY", ":0"), ("TMUX", "x")]),
                settings(Auto, None),
                vec!["xclip", "xsel", "osc52", "stdout"],
            ),
            (
                "x11 over ssh (forwarding)",
                linux_env(&[("DISPLAY", "localhost:10"), ssh]),
                settings(Auto, None),
                vec!["osc52", "stdout"],
            ),
            (
                "x11 over ssh, native forced",
                linux_env(&[("DISPLAY", "localhost:10"), ssh]),
                settings(Native, None),
                vec!["xclip", "xsel", "osc52", "stdout"],
            ),
            (
                "no display",
                linux_env(&[]),
                settings(Auto, None),
                vec!["osc52", "stdout"],
            ),
            (
                "no display, ssh",
                linux_env(&[ssh]),
                settings(Auto, None),
                vec!["osc52", "stdout"],
            ),
            (
                "no display, native forced",
                linux_env(&[]),
                settings(Native, None),
                vec!["osc52", "stdout"],
            ),
            (
                "x11, osc52 forced",
                linux_env(&[("DISPLAY", ":0")]),
                settings(Osc52, None),
                vec!["osc52", "xclip", "xsel", "stdout"],
            ),
            (
                "linux stdout forced",
                linux_env(&[("DISPLAY", ":0")]),
                settings(Stdout, None),
                vec!["stdout"],
            ),
            (
                "custom, auto, local",
                mac_env(&[]),
                settings(Auto, Some("my-copy")),
                vec!["custom", "osc52", "stdout"],
            ),
            (
                "custom, auto, ssh",
                linux_env(&[ssh]),
                settings(Auto, Some("my-copy")),
                vec!["custom", "osc52", "stdout"],
            ),
            (
                "custom, native",
                linux_env(&[("DISPLAY", ":0")]),
                settings(Native, Some("my-copy")),
                vec!["custom", "osc52", "stdout"],
            ),
            (
                "custom, osc52 forced",
                mac_env(&[]),
                settings(Osc52, Some("my-copy")),
                vec!["osc52", "custom", "stdout"],
            ),
            (
                "custom, stdout forced",
                mac_env(&[]),
                settings(Stdout, Some("my-copy")),
                vec!["stdout"],
            ),
        ];
        for (name, env, s, want) in rows {
            assert_eq!(ids(&env, &s), want, "{name}");
        }
    }

    #[test]
    fn every_chain_ends_in_stdout_and_never_repeats_a_method() {
        let envs = [
            mac_env(&[]),
            mac_env(&[("SSH_TTY", "/dev/pts/1")]),
            linux_env(&[]),
            linux_env(&[("DISPLAY", ":0")]),
            linux_env(&[("WAYLAND_DISPLAY", "w"), ("DISPLAY", ":0")]),
        ];
        for env in &envs {
            for choice in [
                ClipboardChoice::Auto,
                ClipboardChoice::Native,
                ClipboardChoice::Osc52,
                ClipboardChoice::Stdout,
            ] {
                for command in [None, Some("c")] {
                    let c = chain(env, &settings(choice, command), &tools_for(env));
                    assert_eq!(c.last(), Some(&Method::Stdout), "{env:?} {choice:?}");
                    for (i, m) in c.iter().enumerate() {
                        assert!(!c[i + 1..].contains(m), "{m:?} repeats in {c:?}");
                    }
                }
            }
        }
    }

    // ---- copy ----

    fn local_mac() -> FakePlatform {
        FakePlatform::new(mac_env(&[])).with_tool("pbcopy", FakeTool::Ok)
    }

    fn run(p: &FakePlatform, s: &ClipboardSettings, text: &str) -> CopyOutcome {
        copy(p, s, text, Place::Cli)
    }

    #[test]
    fn a_native_tool_that_works_is_confirmed() {
        let p = local_mac();
        let o = run(&p, &ClipboardSettings::default(), "select 1\n");
        assert_eq!(o.used, Some(Method::Native(PBCOPY)));
        assert!(o.confirmed && o.sent() && o.attempts.is_empty() && o.refused.is_none());
        assert_eq!((o.text.as_str(), o.bytes, o.lines), ("select 1", 8, 1));
        assert_eq!(
            p.calls(),
            [
                Call::FindTool("pbcopy".into()),
                Call::RunCopyTool {
                    tool: "pbcopy".into(),
                    text: "select 1".into()
                }
            ]
        );
    }

    #[test]
    fn trailing_newlines_are_trimmed_only_when_configured() {
        let p = local_mac();
        let o = run(&p, &ClipboardSettings::default(), "a\nb\n\n");
        assert_eq!((o.text.as_str(), o.lines), ("a\nb", 2));
        let keep = ClipboardSettings {
            trim_trailing_newline: false,
            ..ClipboardSettings::default()
        };
        let o = run(&p, &keep, "a\nb\n");
        assert_eq!((o.text.as_str(), o.lines), ("a\nb\n", 2));
        let o = run(&p, &ClipboardSettings::default(), "a\r\n");
        assert_eq!(o.text, "a", "CRLF too");
    }

    #[test]
    fn a_missing_tool_is_skipped_with_an_install_hint_and_the_terminal_is_used() {
        let p = FakePlatform::new(linux_env(&[("WAYLAND_DISPLAY", "w")]));
        let o = run(&p, &ClipboardSettings::default(), "hello");
        assert_eq!(o.used, Some(Method::Osc52));
        assert!(!o.confirmed && o.sent());
        assert_eq!(o.attempts.len(), 1);
        assert_eq!(o.attempts[0].method, Method::Native(WL_COPY));
        assert_eq!(o.attempts[0].reason, "not found");
        assert_eq!(o.attempts[0].hint.as_deref(), Some("install wl-clipboard"));
        assert_eq!(p.tty_output(), b"\x1b]52;c;aGVsbG8=\x07");
        assert!(
            !p.calls()
                .iter()
                .any(|c| matches!(c, Call::RunCopyTool { .. }))
        );
    }

    #[test]
    fn a_failing_tool_and_a_hung_tool_fall_through_to_the_next() {
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]))
            .with_tool("xclip", FakeTool::Hang)
            .with_tool("xsel", FakeTool::Ok);
        let o = run(&p, &ClipboardSettings::default(), "x");
        assert_eq!(o.used, Some(Method::Native(XSEL)));
        assert!(o.confirmed);
        assert_eq!(o.attempts.len(), 1);
        assert_eq!(o.attempts[0].reason, "timed out after 2 s");
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]))
            .with_tool("xclip", FakeTool::Fail(1))
            .with_tool("xsel", FakeTool::Fail(2));
        let o = run(&p, &ClipboardSettings::default(), "x");
        assert_eq!(o.used, Some(Method::Osc52));
        assert_eq!(o.attempts.len(), 2);
        assert_eq!(o.attempts[0].reason, "exited with status 1");
        assert_eq!(o.attempts[1].reason, "exited with status 2");
    }

    #[test]
    fn a_forced_method_that_fails_still_falls_back() {
        // native forced over ssh: the tool is tried first, then the terminal (clarification 1)
        let p = FakePlatform::new(mac_env(&[("SSH_CONNECTION", "a b c d")]))
            .with_tool("pbcopy", FakeTool::Fail(1));
        let o = run(&p, &settings(ClipboardChoice::Native, None), "hello");
        assert_eq!(o.used, Some(Method::Osc52));
        assert_eq!(o.attempts.len(), 1);
        // osc52 forced with a working tool: the terminal wins, the tool is never run
        let p = local_mac();
        let o = run(&p, &settings(ClipboardChoice::Osc52, None), "hello");
        assert_eq!(o.used, Some(Method::Osc52));
        assert!(
            !p.calls()
                .iter()
                .any(|c| matches!(c, Call::RunCopyTool { .. }))
        );
    }

    #[test]
    fn the_stdout_method_is_a_method_not_a_failure() {
        let p = local_mac();
        let o = run(&p, &settings(ClipboardChoice::Stdout, None), "text\n");
        assert_eq!(o.used, Some(Method::Stdout));
        assert!(o.attempts.is_empty() && !o.confirmed && o.sent());
        assert!(p.calls().is_empty());
    }

    #[test]
    fn nothing_available_prints() {
        // a Linux console: no display, no terminal to write to
        let p = FakePlatform::new(linux_env(&[])).without_terminal();
        let o = run(&p, &ClipboardSettings::default(), "text");
        assert_eq!(o.used, Some(Method::Stdout));
        assert_eq!(o.attempts.len(), 1);
        assert_eq!(o.attempts[0].method, Method::Osc52);
        assert!(
            o.attempts[0].reason.contains("terminal"),
            "{:?}",
            o.attempts[0]
        );
        assert!(
            o.warnings()[0].contains("terminal (OSC 52)"),
            "{:?}",
            o.warnings()
        );
    }

    #[test]
    fn inside_the_ui_stdout_is_never_used() {
        let p = FakePlatform::new(linux_env(&[])).without_terminal();
        let o = copy(&p, &ClipboardSettings::default(), "text", Place::Ui);
        assert_eq!(o.used, None);
        assert!(!o.sent());
        assert!(
            o.message_short().starts_with("Copy failed"),
            "{}",
            o.message_short()
        );
        let o = copy(
            &local_mac(),
            &settings(ClipboardChoice::Stdout, None),
            "x",
            Place::Ui,
        );
        assert_eq!(o.used, None);
        // a working method is used as usual
        let o = copy(&local_mac(), &ClipboardSettings::default(), "x", Place::Ui);
        assert_eq!(o.used, Some(Method::Native(PBCOPY)));
    }

    #[test]
    fn an_oversized_payload_is_refused_and_not_dumped_to_stdout() {
        let p = FakePlatform::new(mac_env(&[("SSH_CONNECTION", "a b c d")]));
        let s = ClipboardSettings {
            osc52_max_bytes: 10,
            ..ClipboardSettings::default()
        };
        let o = run(&p, &s, "this is longer than ten bytes");
        assert_eq!(o.used, None, "nothing delivered");
        assert!(!o.sent());
        assert_eq!(
            o.refused,
            Some(TooLarge {
                bytes: 29,
                limit: 10
            })
        );
        assert!(
            p.tty_output().is_empty(),
            "nothing is written to the terminal"
        );
        let msg = o.message("sumo/x");
        assert!(
            msg.contains("29 bytes") && msg.contains("clipboard.osc52_max_bytes (10)"),
            "{msg}"
        );
        assert!(
            msg.contains("nothing copied") && msg.contains("kb show --raw sumo/x"),
            "{msg}"
        );
        assert!(
            o.message_short().contains("29") && o.message_short().contains("10"),
            "{}",
            o.message_short()
        );
    }

    #[test]
    fn an_oversized_payload_still_goes_to_a_native_tool_that_can_carry_it() {
        let p = local_mac();
        let s = ClipboardSettings {
            osc52_max_bytes: 10,
            choice: ClipboardChoice::Osc52,
            ..ClipboardSettings::default()
        };
        let o = run(&p, &s, "this is longer than ten bytes");
        assert_eq!(o.used, Some(Method::Native(PBCOPY)));
        assert!(o.confirmed);
        assert_eq!(o.attempts.len(), 1);
        assert!(
            o.attempts[0]
                .reason
                .contains("over clipboard.osc52_max_bytes (10)")
        );
    }

    #[test]
    fn a_custom_command_runs_first_and_is_named_by_its_first_word() {
        let p =
            FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("mycopy", FakeTool::Ok);
        let o = run(
            &p,
            &settings(ClipboardChoice::Auto, Some("mycopy --flag")),
            "t",
        );
        assert_eq!(o.used, Some(Method::Custom("mycopy --flag".into())));
        assert!(o.confirmed);
        assert_eq!(o.message("a/b"), "Copied a/b (1 line) via mycopy");
        // a custom command that fails falls back
        let p = FakePlatform::new(linux_env(&[])).with_tool("mycopy", FakeTool::Fail(127));
        let o = run(&p, &settings(ClipboardChoice::Auto, Some("mycopy")), "t");
        assert_eq!(o.used, Some(Method::Osc52));
        assert_eq!(o.attempts[0].reason, "exited with status 127");
    }

    #[test]
    fn the_sequence_is_wrapped_for_tmux_and_screen() {
        let p = FakePlatform::new(linux_env(&[("TMUX", "x"), ("SSH_TTY", "t")]));
        run(&p, &ClipboardSettings::default(), "hello");
        assert_eq!(
            p.tty_output(),
            b"\x1bPtmux;\x1b\x1b]52;c;aGVsbG8=\x07\x1b\\"
        );
        let p = FakePlatform::new(linux_env(&[("STY", "1.pts"), ("SSH_TTY", "t")]));
        run(&p, &ClipboardSettings::default(), "hello");
        assert_eq!(p.tty_output(), b"\x1bP\x1b]52;c;aGVsbG8=\x07\x1b\\");
    }

    #[test]
    fn tmux_that_does_not_forward_is_pointed_out() {
        let p = FakePlatform::new(linux_env(&[("TMUX", "x"), ("SSH_TTY", "t")]))
            .with_tmux_passthrough(false);
        let o = run(&p, &ClipboardSettings::default(), "hello");
        assert_eq!(
            o.used,
            Some(Method::Osc52),
            "it was sent, so it is reported as sent"
        );
        assert!(
            o.notes
                .iter()
                .any(|n| n.contains("set -g allow-passthrough on")),
            "{:?}",
            o.notes
        );
        assert!(o.warnings().iter().any(|w| w.contains("allow-passthrough")));
        let p = FakePlatform::new(linux_env(&[("TMUX", "x"), ("SSH_TTY", "t")]))
            .with_tmux_passthrough(true);
        assert!(run(&p, &ClipboardSettings::default(), "x").notes.is_empty());
    }

    #[test]
    fn the_confirmation_lines_are_exact() {
        let native = run(&local_mac(), &ClipboardSettings::default(), "a\nb\nc");
        assert_eq!(
            native.message("sumo/5xx-by-authorizer"),
            "Copied sumo/5xx-by-authorizer (3 lines) via pbcopy"
        );
        assert_eq!(native.message_short(), "Copied via pbcopy");
        let one = run(&local_mac(), &ClipboardSettings::default(), "a");
        assert_eq!(one.message("x/y"), "Copied x/y (1 line) via pbcopy");

        let ssh = FakePlatform::new(mac_env(&[("SSH_CONNECTION", "a b c d")]));
        let osc = run(&ssh, &ClipboardSettings::default(), "a\nb");
        assert_eq!(
            osc.message("x/y"),
            "Sent x/y (2 lines) via terminal (OSC 52)"
        );
        assert_eq!(osc.message_short(), "Sent via terminal (OSC 52)");

        let forced = run(&local_mac(), &settings(ClipboardChoice::Stdout, None), "a");
        assert_eq!(forced.message("x/y"), "Printed x/y (1 line) to stdout");
        let nothing = run(
            &FakePlatform::new(linux_env(&[])).without_terminal(),
            &ClipboardSettings::default(),
            "a",
        );
        assert_eq!(
            nothing.message("x/y"),
            "Printed x/y (1 line) to stdout; no clipboard method worked"
        );
    }

    #[test]
    fn warnings_say_what_was_skipped_and_what_to_do() {
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]))
            .with_tool("xclip", FakeTool::Fail(1));
        let o = run(&p, &ClipboardSettings::default(), "x");
        let w = o.warnings();
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(w[0], "kb: xclip skipped: exited with status 1");
        assert_eq!(w[1], "kb: xsel skipped: not found (install xclip or xsel)");
    }

    // ---- reading ----

    use super::super::fake::FakeRead;

    fn reader(env: &[(&str, &str)], os_mac: bool) -> FakePlatform {
        FakePlatform::new(if os_mac {
            mac_env(env)
        } else {
            super::super::fake::linux_env(env)
        })
    }

    fn tools_named(env: &Environment) -> Vec<&'static str> {
        read_tools_for(env).iter().map(|t| t.name).collect()
    }

    #[test]
    fn the_read_tool_arguments_are_exact() {
        assert_eq!(PBPASTE.args, &[] as &[&str]);
        assert_eq!(WL_PASTE.args, &[] as &[&str]);
        assert_eq!(XCLIP_OUT.args, ["-selection", "clipboard", "-out"]);
        assert_eq!(XSEL_OUT.args, ["--clipboard", "--output"]);
    }

    #[test]
    fn read_tool_choice_follows_the_environment() {
        assert_eq!(tools_named(&mac_env(&[])), ["pbpaste"]);
        assert_eq!(tools_named(&linux(true, false)), ["wl-paste"]);
        assert_eq!(tools_named(&linux(false, true)), ["xclip", "xsel"]);
        assert_eq!(
            tools_named(&linux(true, true)),
            ["wl-paste", "xclip", "xsel"],
            "XWayland tries wl-paste first"
        );
        assert!(read_tools_for(&linux(false, false)).is_empty());
        for t in [PBPASTE, WL_PASTE, XCLIP_OUT, XSEL_OUT] {
            assert!(!t.install_hint.is_empty());
        }
    }

    #[test]
    fn text_is_read_with_the_first_tool_that_works() {
        let p = reader(&[], true).with_read("pbpaste", FakeRead::Text("select 1\n".into()));
        assert_eq!(read(&p), ReadOutcome::Text("select 1".into()));
    }

    #[test]
    fn over_ssh_nothing_is_tried() {
        let p = reader(&[("SSH_TTY", "/dev/pts/1")], true)
            .with_read("pbpaste", FakeRead::Text("x".into()));
        assert_eq!(read(&p), ReadOutcome::Unavailable(Unavailable::OverSsh));
        assert!(
            p.calls()
                .iter()
                .all(|c| !matches!(c, Call::RunReadTool { .. })),
            "{:?}",
            p.calls()
        );
    }

    #[test]
    fn a_missing_or_hung_or_failing_tool_falls_through_to_the_next() {
        let env = [("WAYLAND_DISPLAY", "w"), ("DISPLAY", ":0")];
        let missing = reader(&env, false).with_read("xclip", FakeRead::Text("from xclip".into()));
        assert_eq!(read(&missing), ReadOutcome::Text("from xclip".into()));
        let hung = reader(&env, false)
            .with_read("wl-paste", FakeRead::Timeout)
            .with_read("xclip", FakeRead::Text("later".into()));
        assert_eq!(read(&hung), ReadOutcome::Text("later".into()));
        let failing = reader(&env, false)
            .with_read("wl-paste", FakeRead::Fail(1))
            .with_read("xclip", FakeRead::Text("after".into()));
        assert_eq!(read(&failing), ReadOutcome::Text("after".into()));
    }

    #[test]
    fn xwayland_tries_wl_paste_first() {
        let env = [("WAYLAND_DISPLAY", "w"), ("DISPLAY", ":0")];
        let p = reader(&env, false)
            .with_read("wl-paste", FakeRead::Text("one".into()))
            .with_read("xclip", FakeRead::Text("two".into()));
        assert_eq!(read(&p), ReadOutcome::Text("one".into()));
        let ran: Vec<_> = p
            .calls()
            .into_iter()
            .filter(|c| matches!(c, Call::RunReadTool { .. }))
            .collect();
        assert_eq!(
            ran,
            [Call::RunReadTool {
                tool: "wl-paste".into()
            }]
        );
    }

    #[test]
    fn no_tool_is_unavailable_with_the_install_hint() {
        let p = reader(&[("WAYLAND_DISPLAY", "w")], false);
        assert_eq!(
            read(&p),
            ReadOutcome::Unavailable(Unavailable::NoTool {
                hint: "install wl-clipboard".into()
            })
        );
        let none = reader(&[], false);
        let ReadOutcome::Unavailable(Unavailable::NoTool { hint }) = read(&none) else {
            panic!()
        };
        assert!(
            hint.contains("wl-clipboard") && hint.contains("xclip"),
            "{hint}"
        );
        let timed_out = reader(&[("DISPLAY", ":0")], false)
            .with_read("xclip", FakeRead::Timeout)
            .with_read("xsel", FakeRead::Timeout);
        assert_eq!(
            read(&timed_out),
            ReadOutcome::Unavailable(Unavailable::TimedOut)
        );
    }

    #[test]
    fn non_text_is_empty() {
        for bytes in [
            vec![0xff, 0xfe, 0x00, 0x01],
            b"abc\0def".to_vec(),
            vec![0x89, b'P', b'N', b'G'],
        ] {
            let p = reader(&[], true).with_read("pbpaste", FakeRead::Bytes(bytes));
            assert_eq!(read(&p), ReadOutcome::Empty);
        }
    }

    #[test]
    fn line_endings_are_normalized_and_blank_text_is_empty() {
        let p = reader(&[], true).with_read("pbpaste", FakeRead::Text("a\r\nb\rc\n\n".into()));
        assert_eq!(read(&p), ReadOutcome::Text("a\nb\nc".into()));
        for blank in ["", "  \n\t\r\n"] {
            let p = reader(&[], true).with_read("pbpaste", FakeRead::Text(blank.into()));
            assert_eq!(read(&p), ReadOutcome::Empty, "{blank:?}");
        }
    }

    #[test]
    fn more_than_two_mebibytes_is_too_large() {
        let big = "a".repeat(super::super::MAX_READ_BYTES + 1);
        let p = reader(&[], true).with_read("pbpaste", FakeRead::Text(big));
        assert_eq!(read(&p), ReadOutcome::Unavailable(Unavailable::TooLarge));
    }

    #[test]
    fn the_last_tool_failing_quietly_means_nothing_is_copied() {
        let wayland =
            reader(&[("WAYLAND_DISPLAY", "w")], false).with_read("wl-paste", FakeRead::Fail(1));
        assert_eq!(read(&wayland), ReadOutcome::Empty);
        let x11 = reader(&[("DISPLAY", ":0")], false)
            .with_read("xclip", FakeRead::Fail(1))
            .with_read("xsel", FakeRead::Fail(1));
        assert_eq!(read(&x11), ReadOutcome::Empty);
    }
}
