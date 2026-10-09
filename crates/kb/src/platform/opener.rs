//! Opening a URL or a file with the system's default application (`open` on macOS, `xdg-open` on
//! Linux, or `opener.command`). No command uses it yet (M7 adds `kb open`); it is here so the
//! platform layer is complete and tested.

use std::io;
use std::path::Path;

use super::{Opened, Platform};

/// Opens `target` (a URL or a path). Over SSH with no display nothing is launched and the target
/// comes back as `Opened::Printed` for the caller to show.
pub fn open_target(
    platform: &impl Platform,
    opener_override: Option<&str>,
    target: &str,
) -> io::Result<Opened> {
    let env = platform.env();
    let opener = platform.opener(opener_override);
    // Say what is missing before the shell reports a bare "127".
    let program = opener.command.split_whitespace().next().unwrap_or_default();
    let nowhere = super::unix::nowhere_to_open(&env);
    if !nowhere && !program.contains('/') && platform.find_tool(program).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{program} not found; install it or set opener.command"),
        ));
    }
    platform.open(target, &opener, &env)
}

pub fn open_url(
    platform: &impl Platform,
    opener_override: Option<&str>,
    url: &str,
) -> io::Result<Opened> {
    open_target(platform, opener_override, url)
}

pub fn open_path(
    platform: &impl Platform,
    opener_override: Option<&str>,
    path: &Path,
) -> io::Result<Opened> {
    open_target(platform, opener_override, &path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::{Call, FakePlatform, FakeTool, linux_env, mac_env};

    #[test]
    fn macos_uses_open_and_linux_uses_xdg_open() {
        let mac = FakePlatform::new(mac_env(&[])).with_tool("open", FakeTool::Ok);
        assert_eq!(
            open_url(&mac, None, "https://example.com").unwrap(),
            Opened::Launched
        );
        assert!(mac.calls().contains(&Call::Open {
            target: "https://example.com".into(),
            opener: "open".into()
        }));
        let linux =
            FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("xdg-open", FakeTool::Ok);
        open_url(&linux, None, "https://example.com").unwrap();
        assert!(linux.calls().contains(&Call::Open {
            target: "https://example.com".into(),
            opener: "xdg-open".into()
        }));
    }

    #[test]
    fn opener_command_overrides_the_default() {
        let p =
            FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("firefox", FakeTool::Ok);
        open_url(&p, Some("firefox --new-tab"), "https://example.com").unwrap();
        assert!(p.calls().contains(&Call::Open {
            target: "https://example.com".into(),
            opener: "firefox --new-tab".into()
        }));
    }

    #[test]
    fn over_ssh_without_a_display_the_url_is_returned_not_opened() {
        let p = FakePlatform::new(linux_env(&[("SSH_CONNECTION", "a b c d")]));
        assert_eq!(
            open_url(&p, None, "https://example.com").unwrap(),
            Opened::Printed("https://example.com".into())
        );
        assert!(
            !p.calls().iter().any(|c| matches!(c, Call::Open { .. })),
            "nothing is launched"
        );
        // not even a missing opener matters there
        assert!(!p.calls().iter().any(|c| matches!(c, Call::FindTool(_))));
    }

    #[test]
    fn over_ssh_with_forwarded_x11_it_still_opens() {
        let p = FakePlatform::new(linux_env(&[
            ("SSH_CONNECTION", "a b c d"),
            ("DISPLAY", "localhost:10"),
        ]))
        .with_tool("xdg-open", FakeTool::Ok);
        assert_eq!(
            open_url(&p, None, "https://example.com").unwrap(),
            Opened::Launched
        );
    }

    #[test]
    fn a_missing_opener_is_named() {
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]));
        let e = open_url(&p, None, "https://example.com").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(e.to_string().contains("xdg-open not found"), "{e}");
    }

    #[test]
    fn files_are_opened_by_path() {
        let p = FakePlatform::new(mac_env(&[])).with_tool("open", FakeTool::Ok);
        open_path(&p, None, Path::new("/Users/u/kb")).unwrap();
        assert!(p.calls().contains(&Call::Open {
            target: "/Users/u/kb".into(),
            opener: "open".into()
        }));
    }
}
