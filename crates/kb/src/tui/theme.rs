//! Colors and styles. Syntax and markdown produce `Class`es; the theme turns them into styles.

use kb_core::config::ThemeChoice;
use kb_core::entry::EntryType;
use ratatui::style::{Color, Modifier, Style};

/// What a piece of text is, independent of any color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Plain,
    Keyword,
    String,
    Number,
    Comment,
    Operator,
    Function,
    Field,
    Type,
    Variable,
    Punctuation,
    Heading,
    Bold,
    Italic,
    Code,
    Link,
    Quote,
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    Dark,
    Light,
    NoColor,
}

/// The environment variables the theme depends on.
#[derive(Debug, Clone, Default)]
pub struct ThemeEnv {
    pub no_color: bool,
    pub colorfgbg: Option<String>,
    pub colorterm: Option<String>,
}

impl ThemeEnv {
    pub fn from_env() -> ThemeEnv {
        ThemeEnv {
            no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            colorfgbg: std::env::var("COLORFGBG").ok(),
            colorterm: std::env::var("COLORTERM").ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: ThemeName,
    truecolor: bool,
}

/// Fixed per type, readable on light and dark backgrounds (ANSI-256 indexes).
pub const TYPE_COLORS: [(EntryType, u8); 7] = [
    (EntryType::Sql, 33),
    (EntryType::Sumo, 208),
    (EntryType::Cw, 106),
    (EntryType::Cmd, 34),
    (EntryType::Prompt, 127),
    (EntryType::Link, 30),
    (EntryType::Code, 131),
];

/// The RGB value of an xterm-256 palette index.
pub fn rgb_of(index: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match index {
        0..=15 => BASE[usize::from(index)],
        16..=231 => {
            let i = index - 16;
            let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
            (level(i / 36), level(i / 6 % 6), level(i % 6))
        }
        _ => {
            let v = 8 + 10 * (index - 232);
            (v, v, v)
        }
    }
}

impl Theme {
    /// Picks the theme: `NO_COLOR` wins, then the configured choice; `auto` reads `COLORFGBG`
    /// (a light background is 7 or 15) and otherwise assumes dark.
    pub fn resolve(choice: ThemeChoice, env: &ThemeEnv) -> Theme {
        let truecolor = env
            .colorterm
            .as_deref()
            .is_some_and(|c| matches!(c, "truecolor" | "24bit"));
        let name = if env.no_color {
            ThemeName::NoColor
        } else {
            match choice {
                ThemeChoice::Dark => ThemeName::Dark,
                ThemeChoice::Light => ThemeName::Light,
                ThemeChoice::Auto => {
                    let bg = env
                        .colorfgbg
                        .as_deref()
                        .and_then(|v| v.rsplit(';').next())
                        .and_then(|b| b.parse::<u8>().ok());
                    if matches!(bg, Some(7 | 15)) {
                        ThemeName::Light
                    } else {
                        ThemeName::Dark
                    }
                }
            }
        };
        Theme { name, truecolor }
    }

    pub fn dark() -> Theme {
        Theme {
            name: ThemeName::Dark,
            truecolor: false,
        }
    }

    pub fn light() -> Theme {
        Theme {
            name: ThemeName::Light,
            truecolor: false,
        }
    }

    pub fn no_color() -> Theme {
        Theme {
            name: ThemeName::NoColor,
            truecolor: false,
        }
    }

    pub fn is_color(&self) -> bool {
        self.name != ThemeName::NoColor
    }

    fn color(&self, index: u8) -> Color {
        if self.truecolor {
            let (r, g, b) = rgb_of(index);
            Color::Rgb(r, g, b)
        } else {
            Color::Indexed(index)
        }
    }

    /// `(dark, light)` picks by theme; `None` for NoColor.
    fn pick(&self, dark: u8, light: u8) -> Option<Color> {
        match self.name {
            ThemeName::Dark => Some(self.color(dark)),
            ThemeName::Light => Some(self.color(light)),
            ThemeName::NoColor => None,
        }
    }

    fn fg(&self, dark: u8, light: u8) -> Style {
        self.pick(dark, light)
            .map_or(Style::default(), |c| Style::default().fg(c))
    }

    pub fn type_color(&self, ty: EntryType) -> Option<Color> {
        if !self.is_color() {
            return None;
        }
        TYPE_COLORS
            .iter()
            .find(|(t, _)| *t == ty)
            .map(|(_, i)| self.color(*i))
    }

    /// The type badge: colored text, bold; with `NO_COLOR` just bold.
    pub fn badge(&self, ty: EntryType) -> Style {
        match self.type_color(ty) {
            Some(c) => Style::default().fg(c).add_modifier(Modifier::BOLD),
            None => Style::default().add_modifier(Modifier::BOLD),
        }
    }

    pub fn text(&self) -> Style {
        Style::default()
    }

    pub fn muted(&self) -> Style {
        match self.name {
            ThemeName::NoColor => Style::default().add_modifier(Modifier::DIM),
            _ => self.fg(245, 241),
        }
    }

    pub fn selection(&self) -> Style {
        match self.pick(237, 254) {
            Some(bg) => Style::default().bg(bg),
            None => Style::default().add_modifier(Modifier::REVERSED),
        }
    }

    pub fn matched(&self) -> Style {
        let base = Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        match self.pick(214, 130) {
            Some(c) => base.fg(c),
            None => base,
        }
    }

    pub fn warning(&self) -> Style {
        let base = Style::default().add_modifier(Modifier::BOLD);
        match self.pick(214, 166) {
            Some(c) => base.fg(c),
            None => base,
        }
    }

    pub fn border(&self) -> Style {
        match self.name {
            ThemeName::NoColor => Style::default(),
            _ => self.fg(240, 246),
        }
    }

    pub fn chip_active(&self) -> Style {
        match (self.pick(24, 153), self.pick(255, 17)) {
            (Some(bg), Some(fg)) => Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
            _ => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        }
    }

    pub fn chip_dim(&self) -> Style {
        match self.name {
            ThemeName::NoColor => Style::default().add_modifier(Modifier::DIM),
            _ => self.fg(240, 248),
        }
    }

    pub fn cursor(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// `{{variable}}` markers.
    pub fn variable(&self) -> Style {
        let base = Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        match self.pick(215, 130) {
            Some(c) => base.fg(c),
            None => base,
        }
    }

    pub fn class_style(&self, class: Class) -> Style {
        let none = !self.is_color();
        let italic = Style::default().add_modifier(Modifier::ITALIC);
        match class {
            Class::Plain => Style::default(),
            Class::Keyword => self.fg(141, 54).add_modifier(Modifier::BOLD),
            Class::String => self.fg(107, 22),
            Class::Number => self.fg(173, 130),
            Class::Comment => self.fg(244, 244).add_modifier(
                Modifier::ITALIC
                    | if none {
                        Modifier::DIM
                    } else {
                        Modifier::empty()
                    },
            ),
            Class::Operator | Class::Punctuation => self.fg(250, 240),
            Class::Function => self.fg(110, 25),
            Class::Field => self.fg(179, 94),
            Class::Type => self.fg(79, 30),
            Class::Variable => self.variable(),
            Class::Heading => Style::default().add_modifier(Modifier::BOLD),
            Class::Bold => Style::default().add_modifier(Modifier::BOLD),
            Class::Italic => italic,
            Class::Code => self.fg(180, 94),
            Class::Link => self.fg(74, 25).add_modifier(Modifier::UNDERLINED),
            Class::Quote => self.muted().add_modifier(Modifier::ITALIC),
            Class::Muted => self.muted(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(no_color: bool, fgbg: Option<&str>, term: Option<&str>) -> ThemeEnv {
        ThemeEnv {
            no_color,
            colorfgbg: fgbg.map(str::to_string),
            colorterm: term.map(str::to_string),
        }
    }

    #[test]
    fn no_color_wins_over_everything() {
        for choice in [ThemeChoice::Auto, ThemeChoice::Dark, ThemeChoice::Light] {
            assert_eq!(
                Theme::resolve(choice, &env(true, Some("0;15"), None)).name,
                ThemeName::NoColor
            );
        }
        assert_eq!(
            Theme::resolve(ThemeChoice::Auto, &env(false, None, None)).name,
            ThemeName::Dark
        );
    }

    #[test]
    fn auto_reads_colorfgbg() {
        let t = |v| Theme::resolve(ThemeChoice::Auto, &env(false, v, None)).name;
        assert_eq!(t(Some("0;15")), ThemeName::Light);
        assert_eq!(t(Some("0;default;15")), ThemeName::Light);
        assert_eq!(t(Some("15;0")), ThemeName::Dark);
        assert_eq!(t(Some("0;7")), ThemeName::Light);
        assert_eq!(t(Some("garbage")), ThemeName::Dark);
        assert_eq!(t(None), ThemeName::Dark);
        let fixed = |c, v| Theme::resolve(c, &env(false, Some(v), None)).name;
        assert_eq!(fixed(ThemeChoice::Dark, "0;15"), ThemeName::Dark);
        assert_eq!(fixed(ThemeChoice::Light, "15;0"), ThemeName::Light);
    }

    #[test]
    fn type_colors_are_the_documented_ones_in_every_color_theme() {
        let expect = [
            (EntryType::Sql, 33u8),
            (EntryType::Sumo, 208),
            (EntryType::Cw, 106),
            (EntryType::Cmd, 34),
            (EntryType::Prompt, 127),
            (EntryType::Link, 30),
            (EntryType::Code, 131),
        ];
        for theme in [Theme::dark(), Theme::light()] {
            for (ty, idx) in expect {
                assert_eq!(theme.type_color(ty), Some(Color::Indexed(idx)), "{ty:?}");
            }
        }
        for (ty, _) in expect {
            assert_eq!(Theme::no_color().type_color(ty), None);
            assert!(
                Theme::no_color()
                    .badge(ty)
                    .add_modifier
                    .contains(Modifier::BOLD)
            );
        }
    }

    #[test]
    fn dark_and_light_differ_and_no_color_has_no_colors() {
        assert_ne!(
            Theme::dark().class_style(Class::Keyword),
            Theme::light().class_style(Class::Keyword)
        );
        let nc = Theme::no_color();
        let all = [
            Class::Plain,
            Class::Keyword,
            Class::String,
            Class::Number,
            Class::Comment,
            Class::Operator,
            Class::Function,
            Class::Field,
            Class::Type,
            Class::Variable,
            Class::Punctuation,
            Class::Heading,
            Class::Bold,
            Class::Italic,
            Class::Code,
            Class::Link,
            Class::Quote,
            Class::Muted,
        ];
        for c in all {
            let s = nc.class_style(c);
            assert_eq!((s.fg, s.bg), (None, None), "{c:?}");
        }
        for s in [
            nc.selection(),
            nc.matched(),
            nc.warning(),
            nc.border(),
            nc.chip_active(),
            nc.chip_dim(),
            nc.muted(),
            nc.variable(),
        ] {
            assert_eq!((s.fg, s.bg), (None, None));
        }
        assert!(nc.selection().add_modifier.contains(Modifier::REVERSED));
        assert!(
            nc.matched()
                .add_modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert!(
            nc.variable()
                .add_modifier
                .contains(Modifier::BOLD | Modifier::REVERSED)
        );
        assert!(Theme::dark().selection().bg.is_some());
    }

    #[test]
    fn truecolor_uses_rgb_otherwise_indexes() {
        let t = Theme::resolve(ThemeChoice::Dark, &env(false, None, Some("truecolor")));
        assert!(matches!(t.type_color(EntryType::Sql), Some(Color::Rgb(..))));
        let t = Theme::resolve(ThemeChoice::Dark, &env(false, None, Some("xterm")));
        assert_eq!(t.type_color(EntryType::Sql), Some(Color::Indexed(33)));
        assert_eq!(rgb_of(0), (0, 0, 0));
        assert_eq!(rgb_of(196), (255, 0, 0));
        assert_eq!(rgb_of(232), (8, 8, 8));
        assert_eq!(rgb_of(255), (238, 238, 238));
    }
}
