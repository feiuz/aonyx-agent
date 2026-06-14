//! Welcome-screen otter for the interactive TUI.
//!
//! A single front-facing otter; at runtime the pupils follow the cursor, the
//! whiskers sway and the ears twitch (driven by the TUI tick). Data-only — the
//! animation lives in `tui::TuiApp::render_welcome`. Baked from the Aonyx otter
//! icon (luminance ramp).

/// The otter art, front-facing. Eye sockets are redrawn at runtime so the
/// pupils can look toward the cursor — see [`EYES`].
pub const OTTER: &str = r#"                   .:--====--:.
              .-+*#**++====++**#*+-.
       =**##**#*=:              .-*#***#**=.
     :%#-. .::                      ::. .:*@-
     *@.***+:                        .+**#.%#
     -@=:+%:                          .#+--@=
      -%*:      .                .      :*%-
       .@*                              +@.
       .@=    ( @ ).:--------: ( @ )    -@:
       .@=         =-:-=++=-:-=         -@:
        %%.......   :@@@@@@@@-   .......#%
   .:--=*@%=--===:   -+#@@#+-   :===---%@*=--:.
   .-====+@%*==+==.    :@@:    .==+=-*%@+====-:
 .-=--==+#%#%##%**=---======---=**%##%#%#+==--=-.
 .:  :=*%+-=+-=*####*+:.   :+*####*+-+=-+%*=-  .:
     +%+:==:     .::-=*####*+-::.     :==:+%+
   :%#.:+:                              :+-.#%:
  -@+  .                                  .  +@=
 -@=                                          =@=
 :-                                            ::"#;

/// Eye socket centres `(col, row)` inside [`OTTER`]. The pupil `@` is drawn at
/// the centre, offset toward the cursor; a blink draws `---` instead.
pub const EYES: [(u16, u16); 2] = [(16, 8), (33, 8)];

/// Direction from the otter centre to the mouse as `(dx, dy)` each in `-1..=1`,
/// with a deadzone (cells) that keeps the gaze centred near the middle.
pub fn direction(mouse: (u16, u16), center: (u16, u16), dz: u16) -> (i8, i8) {
    let sgn = |a: u16, b: u16| -> i8 {
        if a > b.saturating_add(dz) {
            1
        } else if b > a.saturating_add(dz) {
            -1
        } else {
            0
        }
    };
    (sgn(mouse.0, center.0), sgn(mouse.1, center.1))
}
