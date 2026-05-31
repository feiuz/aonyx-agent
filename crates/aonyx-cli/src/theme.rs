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

/// Labels of the editable scalar colour fields, in panel order
/// (Phase KK). `accents` + `name` are not directly editable.
pub const EDITABLE_FIELDS: [&str; 10] = [
    "header_fg",
    "composer_border",
    "suggestion_border",
    "status_bg",
    "status_fg",
    "user_prefix",
    "assistant_prefix",
    "thinking",
    "dim",
    "status_busy_bg",
];

impl Theme {
    /// Read the colour of the editable field at `idx` (see
    /// [`EDITABLE_FIELDS`]). Out-of-range → `Color::Reset`.
    pub fn field_color(&self, idx: usize) -> Color {
        match idx {
            0 => self.header_fg,
            1 => self.composer_border,
            2 => self.suggestion_border,
            3 => self.status_bg,
            4 => self.status_fg,
            5 => self.user_prefix,
            6 => self.assistant_prefix,
            7 => self.thinking,
            8 => self.dim,
            9 => self.status_busy_bg,
            _ => Color::Reset,
        }
    }

    /// Set the editable field at `idx` to `c` (Phase KK).
    pub fn set_field(&mut self, idx: usize, c: Color) {
        match idx {
            0 => self.header_fg = c,
            1 => self.composer_border = c,
            2 => self.suggestion_border = c,
            3 => self.status_bg = c,
            4 => self.status_fg = c,
            5 => self.user_prefix = c,
            6 => self.assistant_prefix = c,
            7 => self.thinking = c,
            8 => self.dim = c,
            9 => self.status_busy_bg = c,
            _ => {}
        }
    }

    /// Snapshot the ten editable fields as RGB triples, in
    /// [`EDITABLE_FIELDS`] order (Phase KK) — for serialization.
    pub fn editable_rgb(&self) -> [(u8, u8, u8); 10] {
        let mut out = [(0u8, 0u8, 0u8); 10];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = color_to_rgb(self.field_color(i));
        }
        out
    }
}

/// Build a `custom` theme from ten RGB triples in [`EDITABLE_FIELDS`]
/// order (Phase KK). Accents are derived from the chosen accent-ish
/// colours so the spinner stays coherent; the slice is leaked once to
/// satisfy the `&'static` field (a single tiny per-process leak).
pub fn from_rgb_fields(fields: &[(u8, u8, u8); 10]) -> Theme {
    let c = |i: usize| {
        let (r, g, b) = fields[i];
        Color::Rgb(r, g, b)
    };
    // accents: assistant_prefix, suggestion_border, composer_border, user_prefix.
    let accents: &'static [Color] = Box::leak(vec![c(6), c(2), c(1), c(5)].into_boxed_slice());
    Theme {
        name: "custom",
        header_fg: c(0),
        composer_border: c(1),
        suggestion_border: c(2),
        status_bg: c(3),
        status_fg: c(4),
        user_prefix: c(5),
        assistant_prefix: c(6),
        accents,
        thinking: c(7),
        dim: c(8),
        status_busy_bg: c(9),
    }
}

/// Resolve any [`Color`] to an `(r, g, b)` triple. Named ANSI colours
/// use conventional xterm approximations so the editor can work purely
/// in RGB space (Phase KK).
pub fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 0, 0),
        Color::Green => (0, 205, 0),
        Color::Yellow => (205, 205, 0),
        Color::Blue => (0, 0, 238),
        Color::Magenta => (205, 0, 205),
        Color::Cyan => (0, 205, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (127, 127, 127),
        Color::LightRed => (255, 0, 0),
        Color::LightGreen => (0, 255, 0),
        Color::LightYellow => (255, 255, 0),
        Color::LightBlue => (92, 92, 255),
        Color::LightMagenta => (255, 0, 255),
        Color::LightCyan => (0, 255, 255),
        Color::White => (255, 255, 255),
        // Indexed / Reset have no fixed RGB — use a neutral mid-gray.
        _ => (128, 128, 128),
    }
}

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

    #[test]
    fn color_to_rgb_resolves_named_and_rgb() {
        assert_eq!(color_to_rgb(Color::Rgb(1, 2, 3)), (1, 2, 3));
        assert_eq!(color_to_rgb(Color::White), (255, 255, 255));
        assert_eq!(color_to_rgb(Color::Black), (0, 0, 0));
        // Indexed has no fixed RGB → neutral gray.
        assert_eq!(color_to_rgb(Color::Indexed(42)), (128, 128, 128));
    }

    #[test]
    fn field_get_set_round_trips() {
        let mut t = DEFAULT;
        t.set_field(0, Color::Rgb(10, 20, 30));
        assert_eq!(t.field_color(0), Color::Rgb(10, 20, 30));
        // Out-of-range set is a no-op; get returns Reset.
        t.set_field(99, Color::Rgb(1, 1, 1));
        assert_eq!(t.field_color(99), Color::Reset);
    }

    #[test]
    fn editable_rgb_has_one_entry_per_field() {
        let rgb = DEFAULT.editable_rgb();
        assert_eq!(rgb.len(), EDITABLE_FIELDS.len());
        // header_fg is White in DEFAULT.
        assert_eq!(rgb[0], (255, 255, 255));
    }

    #[test]
    fn from_rgb_fields_builds_a_custom_theme() {
        let fields = [(10, 20, 30); 10];
        let t = from_rgb_fields(&fields);
        assert_eq!(t.name, "custom");
        assert_eq!(t.header_fg, Color::Rgb(10, 20, 30));
        assert!(!t.accents.is_empty());
    }
}
