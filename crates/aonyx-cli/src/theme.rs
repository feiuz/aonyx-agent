//! Visual themes for the TUI.
//!
//! Each theme is a flat bag of `ratatui` colours used by `tui::render`.
//! Four are bundled at compile time (`default`, `catppuccin`, `dracula`,
//! `gruvbox`); a user can switch live with `/themes <name>` or set the
//! default in `~/.aonyx/config.toml::theme`.

use ratatui::style::Color;

/// Flat theme description consumed by the renderer.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Display name.
    pub name: &'static str,
    /// Header text colour ("🦦 Aonyx Agent").
    pub header_fg: Color,
    /// Composer border when idle.
    pub composer_border: Color,
    /// Suggestion popup border.
    pub suggestion_border: Color,
    /// Status bar background.
    pub status_bg: Color,
    /// Status bar foreground (idle).
    pub status_fg: Color,
    /// `you>` prefix.
    pub user_prefix: Color,
    /// `aonyx>` prefix.
    pub assistant_prefix: Color,
    /// Cycled palette for the spinner + header pulse during runner_active.
    pub accents: &'static [Color],
    /// Thinking placeholder colour.
    pub thinking: Color,
    /// Dim / secondary text (tool results, hints).
    pub dim: Color,
    /// Background for the status bar when the runner is busy.
    pub status_busy_bg: Color,
}

pub const DEFAULT: Theme = Theme {
    name: "default",
    header_fg: Color::White,
    composer_border: Color::Cyan,
    suggestion_border: Color::Magenta,
    status_bg: Color::Rgb(20, 30, 40),
    status_fg: Color::Gray,
    user_prefix: Color::Green,
    assistant_prefix: Color::Magenta,
    accents: &[Color::Magenta, Color::Cyan, Color::LightBlue, Color::Cyan],
    thinking: Color::Magenta,
    dim: Color::DarkGray,
    status_busy_bg: Color::Rgb(20, 20, 50),
};

pub const CATPPUCCIN_MOCHA: Theme = Theme {
    name: "catppuccin",
    header_fg: Color::Rgb(245, 224, 220),         // rosewater
    composer_border: Color::Rgb(137, 180, 250),   // blue
    suggestion_border: Color::Rgb(203, 166, 247), // mauve
    status_bg: Color::Rgb(24, 24, 37),            // mantle
    status_fg: Color::Rgb(186, 194, 222),         // text muted
    user_prefix: Color::Rgb(166, 227, 161),       // green
    assistant_prefix: Color::Rgb(203, 166, 247),  // mauve
    accents: &[
        Color::Rgb(203, 166, 247), // mauve
        Color::Rgb(137, 180, 250), // blue
        Color::Rgb(116, 199, 236), // sapphire
        Color::Rgb(148, 226, 213), // teal
    ],
    thinking: Color::Rgb(245, 194, 231),    // pink
    dim: Color::Rgb(108, 112, 134),         // surface2
    status_busy_bg: Color::Rgb(30, 30, 46), // base
};

pub const DRACULA: Theme = Theme {
    name: "dracula",
    header_fg: Color::Rgb(248, 248, 242),         // foreground
    composer_border: Color::Rgb(139, 233, 253),   // cyan
    suggestion_border: Color::Rgb(255, 121, 198), // pink
    status_bg: Color::Rgb(40, 42, 54),            // background
    status_fg: Color::Rgb(189, 147, 249),         // purple
    user_prefix: Color::Rgb(80, 250, 123),        // green
    assistant_prefix: Color::Rgb(189, 147, 249),  // purple
    accents: &[
        Color::Rgb(255, 121, 198), // pink
        Color::Rgb(189, 147, 249), // purple
        Color::Rgb(139, 233, 253), // cyan
        Color::Rgb(241, 250, 140), // yellow
    ],
    thinking: Color::Rgb(255, 121, 198),    // pink
    dim: Color::Rgb(98, 114, 164),          // comment
    status_busy_bg: Color::Rgb(68, 71, 90), // current line
};

pub const GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox",
    header_fg: Color::Rgb(235, 219, 178),         // fg
    composer_border: Color::Rgb(131, 165, 152),   // aqua
    suggestion_border: Color::Rgb(211, 134, 155), // pink
    status_bg: Color::Rgb(40, 40, 40),            // bg0
    status_fg: Color::Rgb(168, 153, 132),         // fg4
    user_prefix: Color::Rgb(184, 187, 38),        // green
    assistant_prefix: Color::Rgb(211, 134, 155),  // pink
    accents: &[
        Color::Rgb(254, 128, 25),  // orange
        Color::Rgb(250, 189, 47),  // yellow
        Color::Rgb(184, 187, 38),  // green
        Color::Rgb(131, 165, 152), // aqua
    ],
    thinking: Color::Rgb(254, 128, 25),     // orange
    dim: Color::Rgb(146, 131, 116),         // gray
    status_busy_bg: Color::Rgb(60, 56, 54), // bg1
};

const ALL: &[Theme] = &[DEFAULT, CATPPUCCIN_MOCHA, DRACULA, GRUVBOX_DARK];

/// List the names of every bundled theme.
pub fn available_names() -> Vec<&'static str> {
    ALL.iter().map(|t| t.name).collect()
}

/// Resolve a theme by name; falls back to [`DEFAULT`] when unknown.
pub fn by_name(name: &str) -> Theme {
    let lower = name.trim().to_lowercase();
    for t in ALL {
        if t.name.eq_ignore_ascii_case(&lower) {
            return *t;
        }
    }
    DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_names_lists_every_theme() {
        let names = available_names();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"catppuccin"));
        assert!(names.contains(&"dracula"));
        assert!(names.contains(&"gruvbox"));
    }

    #[test]
    fn by_name_resolves_known_themes() {
        assert_eq!(by_name("dracula").name, "dracula");
        assert_eq!(by_name("DRACULA").name, "dracula");
        assert_eq!(by_name(" gruvbox ").name, "gruvbox");
    }

    #[test]
    fn by_name_falls_back_to_default_for_unknown() {
        assert_eq!(by_name("nope").name, "default");
    }
}
