//! What kind of machine and terminal kb is running on, as plain values. `detect` is a pure
//! function of an environment lookup, so every combination is tested on any host.

/// What to tell a user whose terminal does not advertise 24-bit color. The variable is named here,
/// in the platform layer, because reading and naming environment variables is its job.
pub const TRUE_COLOR_ADVICE: &str = "set COLORTERM=truecolor if your terminal supports it";
pub const TRUE_COLOR_ADVERTISED: &str = "advertised (COLORTERM)";

/// The operating system kb was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Os::MacOs => "macOS",
            Os::Linux => "Linux",
        }
    }
}

/// How graphical clipboard tools reach the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// macOS: the window server is always there.
    Quartz,
    Wayland,
    X11,
    None,
}

impl Display {
    pub fn name(self) -> &'static str {
        match self {
            Display::Quartz => "macOS",
            Display::Wayland => "Wayland",
            Display::X11 => "X11",
            Display::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub os: Os,
    pub arch: String,
    pub display: Display,
    /// `DISPLAY` is set (on Wayland this is XWayland, so X11 tools still work).
    pub x11: bool,
    pub ssh: bool,
    pub tmux: bool,
    /// GNU screen (and not tmux, which sets `TERM=screen*` too).
    pub screen: bool,
    pub true_color: bool,
    pub term: Option<String>,
    pub term_program: Option<String>,
}

impl Environment {
    /// Reads the variables through `get`; an empty value counts as unset.
    pub fn detect(get: &dyn Fn(&str) -> Option<String>, os: Os, arch: &str) -> Environment {
        let var = |name: &str| get(name).filter(|v| !v.is_empty());
        let set = |name: &str| var(name).is_some();
        let tmux = set("TMUX");
        let display = match os {
            Os::MacOs => Display::Quartz,
            Os::Linux if set("WAYLAND_DISPLAY") => Display::Wayland,
            Os::Linux if set("DISPLAY") => Display::X11,
            Os::Linux => Display::None,
        };
        Environment {
            os,
            arch: arch.to_string(),
            display,
            x11: os == Os::Linux && set("DISPLAY"),
            ssh: set("SSH_CONNECTION") || set("SSH_TTY") || set("SSH_CLIENT"),
            tmux,
            screen: set("STY") && !tmux,
            true_color: var("COLORTERM")
                .is_some_and(|c| c.contains("truecolor") || c.contains("24bit")),
            term: var("TERM"),
            term_program: var("TERM_PROGRAM"),
        }
    }

    /// The environment of this process.
    pub fn current() -> Environment {
        Environment::detect(
            &|name| std::env::var(name).ok(),
            Os::current(),
            std::env::consts::ARCH,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(os: Os, vars: &[(&str, &str)]) -> Environment {
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
    fn nothing_set_is_a_plain_local_terminal() {
        let e = detect(Os::Linux, &[]);
        assert_eq!(e.display, Display::None);
        assert!(!e.ssh && !e.tmux && !e.screen && !e.true_color);
        assert_eq!(
            (e.term, e.term_program, e.arch.as_str()),
            (None, None, "x86_64")
        );
    }

    #[test]
    fn any_ssh_variable_means_ssh() {
        for var in ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"] {
            assert!(detect(Os::Linux, &[(var, "1.2.3.4 22")]).ssh, "{var}");
        }
        assert!(
            !detect(Os::Linux, &[("SSH_CONNECTION", "")]).ssh,
            "empty is unset"
        );
    }

    #[test]
    fn tmux_and_screen_are_told_apart() {
        let e = detect(Os::Linux, &[("TMUX", "/tmp/tmux-1/default,1,0")]);
        assert!(e.tmux && !e.screen);
        let e = detect(Os::Linux, &[("STY", "123.pts-0.host")]);
        assert!(e.screen && !e.tmux);
        let e = detect(Os::Linux, &[("TMUX", "x"), ("STY", "123.pts-0.host")]);
        assert!(e.tmux && !e.screen, "tmux inside screen wins");
    }

    #[test]
    fn linux_prefers_wayland_over_x11() {
        assert_eq!(
            detect(Os::Linux, &[("DISPLAY", ":0")]).display,
            Display::X11
        );
        assert_eq!(
            detect(Os::Linux, &[("WAYLAND_DISPLAY", "wayland-0")]).display,
            Display::Wayland
        );
        let both = detect(
            Os::Linux,
            &[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")],
        );
        assert_eq!(both.display, Display::Wayland);
        assert_eq!(detect(Os::Linux, &[("DISPLAY", "")]).display, Display::None);
        assert!(both.x11, "XWayland: X11 tools still apply under Wayland");
        assert!(!detect(Os::Linux, &[("WAYLAND_DISPLAY", "wayland-0")]).x11);
        assert!(!detect(Os::MacOs, &[("DISPLAY", ":0")]).x11);
    }

    #[test]
    fn macos_always_has_a_display() {
        assert_eq!(detect(Os::MacOs, &[]).display, Display::Quartz);
        assert_eq!(
            detect(Os::MacOs, &[("DISPLAY", ":0")]).display,
            Display::Quartz
        );
    }

    #[test]
    fn true_color_is_advertised_through_colorterm() {
        assert!(detect(Os::Linux, &[("COLORTERM", "truecolor")]).true_color);
        assert!(detect(Os::Linux, &[("COLORTERM", "24bit")]).true_color);
        assert!(!detect(Os::Linux, &[("COLORTERM", "256color")]).true_color);
        assert!(!detect(Os::Linux, &[("TERM", "xterm-256color")]).true_color);
    }

    #[test]
    fn term_values_are_carried_for_hints() {
        let e = detect(
            Os::MacOs,
            &[
                ("TERM", "xterm-256color"),
                ("TERM_PROGRAM", "Apple_Terminal"),
            ],
        );
        assert_eq!(e.term.as_deref(), Some("xterm-256color"));
        assert_eq!(e.term_program.as_deref(), Some("Apple_Terminal"));
    }

    #[test]
    fn the_names_are_for_people() {
        assert_eq!(Os::MacOs.name(), "macOS");
        assert_eq!(Display::Wayland.name(), "Wayland");
        assert_eq!(Display::None.name(), "none");
    }
}
