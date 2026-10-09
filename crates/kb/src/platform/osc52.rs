//! The OSC 52 escape sequence: asks the terminal to put text on its clipboard. It works over SSH
//! because the terminal, not kb, does the copying. Nothing here writes anywhere; callers send the
//! bytes to the controlling terminal (`Platform::write_tty`).

use super::Environment;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// GNU screen limits the length of one DCS string, so the sequence is cut into pieces.
const SCREEN_CHUNK: usize = 250;

/// RFC 4648 base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(BASE64[(n >> 18) as usize & 63] as char);
        out.push(BASE64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// How the sequence has to be wrapped to get past a terminal multiplexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    None,
    Tmux,
    Screen,
}

impl Wrap {
    pub fn for_env(env: &Environment) -> Wrap {
        if env.tmux {
            Wrap::Tmux
        } else if env.screen {
            Wrap::Screen
        } else {
            Wrap::None
        }
    }
}

/// The text is longer than the configured limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    pub bytes: usize,
    pub limit: usize,
}

/// The sequence for `text`. The limit counts bytes of text (not of base64).
pub fn sequence(text: &str, wrap: Wrap, max_bytes: usize) -> Result<String, TooLarge> {
    if text.len() > max_bytes {
        return Err(TooLarge {
            bytes: text.len(),
            limit: max_bytes,
        });
    }
    let osc = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    Ok(match wrap {
        Wrap::None => osc,
        // tmux forwards a DCS "tmux;" string with every ESC inside doubled
        Wrap::Tmux => format!("\x1bPtmux;{}\x1b\\", osc.replace('\x1b', "\x1b\x1b")),
        // the OSC is pure ASCII, so cutting at any byte is safe
        Wrap::Screen => {
            let pieces: Vec<&str> = osc
                .as_bytes()
                .chunks(SCREEN_CHUNK)
                .map(|c| std::str::from_utf8(c).unwrap_or_default())
                .collect();
            format!("\x1bP{}\x1b\\", pieces.join("\x1b\\\x1bP"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;

    #[test]
    fn base64_matches_rfc_4648() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), enc);
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn the_plain_sequence() {
        assert_eq!(
            sequence("hello", Wrap::None, 74_000).unwrap(),
            "\x1b]52;c;aGVsbG8=\x07"
        );
    }

    #[test]
    fn tmux_gets_a_passthrough_with_doubled_escapes() {
        assert_eq!(
            sequence("hello", Wrap::Tmux, 74_000).unwrap(),
            "\x1bPtmux;\x1b\x1b]52;c;aGVsbG8=\x07\x1b\\"
        );
    }

    #[test]
    fn a_short_payload_in_screen_is_one_dcs_string() {
        assert_eq!(
            sequence("hello", Wrap::Screen, 74_000).unwrap(),
            "\x1bP\x1b]52;c;aGVsbG8=\x07\x1b\\"
        );
    }

    #[test]
    fn a_long_payload_in_screen_is_cut_into_pieces_that_rejoin_to_the_original() {
        let text = "x".repeat(2_000);
        let s = sequence(&text, Wrap::Screen, 74_000).unwrap();
        assert!(s.starts_with("\x1bP") && s.ends_with("\x1b\\"));
        let inner = &s[2..s.len() - 2];
        let pieces: Vec<&str> = inner.split("\x1b\\\x1bP").collect();
        assert!(pieces.len() > 1);
        assert!(
            pieces.iter().all(|p| p.len() <= SCREEN_CHUNK),
            "{:?}",
            pieces.iter().map(|p| p.len()).max()
        );
        assert_eq!(
            pieces.concat(),
            sequence(&text, Wrap::None, 74_000).unwrap()
        );
    }

    #[test]
    fn the_limit_counts_text_bytes() {
        assert!(sequence(&"x".repeat(74_000), Wrap::None, 74_000).is_ok());
        assert_eq!(
            sequence(&"x".repeat(74_001), Wrap::None, 74_000),
            Err(TooLarge {
                bytes: 74_001,
                limit: 74_000
            })
        );
        // three bytes per character: 2 characters fit in 6, not in 5
        assert!(sequence("日本", Wrap::None, 6).is_ok());
        assert!(sequence("日本", Wrap::None, 5).is_err());
        assert!(sequence("日本語 🚀", Wrap::None, 74_000).is_ok());
    }

    #[test]
    fn the_wrapper_follows_the_multiplexer() {
        let env = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
            Environment::detect(
                &move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
                Os::Linux,
                "x86_64",
            )
        };
        assert_eq!(Wrap::for_env(&env(&[])), Wrap::None);
        assert_eq!(Wrap::for_env(&env(&[("TMUX", "x")])), Wrap::Tmux);
        assert_eq!(Wrap::for_env(&env(&[("STY", "x")])), Wrap::Screen);
        assert_eq!(
            Wrap::for_env(&env(&[("TMUX", "x"), ("STY", "y")])),
            Wrap::Tmux
        );
    }
}
