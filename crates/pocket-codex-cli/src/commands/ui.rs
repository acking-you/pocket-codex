//! Terminal output styling for the `pocket-codex` CLI.
//!
//! This module is the single source of truth for how user-facing
//! command output looks: colored status glyphs, aligned key/value
//! fields and boxed grid tables. Everything degrades to plain,
//! script-safe text when stdout is not a TTY or `NO_COLOR` is set, so
//! piping the output into another program never leaks ANSI escapes.
//!
//! Glyphs are kept in non-color mode because they are Unicode
//! characters, not escape codes — only the ANSI coloring is dropped.

use std::{
    env,
    io::{stderr, stdout, IsTerminal},
    sync::OnceLock,
};

use comfy_table::{presets, Attribute, Cell, Color, ContentArrangement, Table};
use owo_colors::{OwoColorize, Style};
use pocket_codex_core::service::ServiceKind;

/// Whether colored output should be emitted on stdout.
///
/// True only when stdout is a terminal and `NO_COLOR` is unset. Cached
/// in a [`OnceLock`] because the answer cannot change within one
/// process run.
pub(crate) fn use_color() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| color_allowed() && stdout().is_terminal())
}

/// Whether colored output should be emitted on stderr.
///
/// Tracked independently of stdout so a warning keeps its color even
/// when stdout is piped but stderr is still attached to the terminal.
pub(crate) fn use_color_stderr() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| color_allowed() && stderr().is_terminal())
}

/// Honor the `NO_COLOR` convention: any non-empty value disables color.
fn color_allowed() -> bool {
    env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

/// Semantic category of a status line, mapped to a glyph + color.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Tone {
    /// A successful, settled outcome (green `✓`).
    Ok,
    /// A next step the user should take (cyan `→`).
    Action,
    /// A state change such as replacing a stale worker (yellow `↻`).
    Change,
    /// A low-emphasis, no-op outcome (dim `·`).
    Muted,
}

impl Tone {
    /// Leading glyph for this tone. Unicode, so it survives non-color mode.
    fn glyph(self) -> &'static str {
        match self {
            Tone::Ok => "✓",
            Tone::Action => "→",
            Tone::Change => "↻",
            Tone::Muted => "·",
        }
    }

    /// Base color style for this tone.
    fn style(self) -> Style {
        match self {
            Tone::Ok => Style::new().green(),
            Tone::Action => Style::new().cyan(),
            Tone::Change => Style::new().yellow(),
            Tone::Muted => Style::new().dimmed(),
        }
    }
}

/// Apply `style` to `text` on stdout, or return it verbatim when color
/// is disabled. Returns an owned `String` so callers stay borrow-free.
fn paint(text: &str, style: Style) -> String {
    if use_color() {
        text.style(style).to_string()
    } else {
        text.to_string()
    }
}

/// Print a status headline: a colored glyph followed by a bold title.
pub(crate) fn headline(tone: Tone, title: &str) {
    let glyph = paint(tone.glyph(), tone.style());
    let title = paint(title, tone.style().bold());
    println!("{glyph} {title}");
}

/// Print an indented `label`/`value` pair beneath a [`headline`]. The
/// label is padded to a fixed width *before* dimming so columns align.
pub(crate) fn field(label: &str, value: &str) {
    let label = paint(&format!("{label:<10}"), Style::new().dimmed());
    println!("    {label}{value}");
}

/// Print an indented, copy-pasteable command line in cyan.
pub(crate) fn code(line: &str) {
    println!("    {}", paint(line, Style::new().cyan()));
}

/// Print a yellow warning to stderr, gated on the stderr color check.
pub(crate) fn warn(msg: &str) {
    let body = format!("⚠ {msg}");
    if use_color_stderr() {
        eprintln!("{}", body.yellow());
    } else {
        eprintln!("{body}");
    }
}

/// Print a bold banner with an optional dim "· subtitle" and a trailing
/// blank line. Used as the heading of a multi-row view.
pub(crate) fn banner(title: &str, subtitle: Option<&str>) {
    let title = paint(title, Style::new().bold());
    match subtitle {
        Some(sub) => {
            let sub = paint(&format!("· {sub}"), Style::new().dimmed());
            println!("{title} {sub}");
        },
        None => println!("{title}"),
    }
    println!();
}

/// Print a dimmed `label  value` footer line (e.g. the shared log dir).
pub(crate) fn footer(label: &str, value: &str) {
    println!("{}", paint(&format!("{label}  {value}"), Style::new().dimmed()));
}

/// Print a standalone dimmed line (e.g. an empty-state notice).
pub(crate) fn muted(msg: &str) {
    println!("{}", paint(msg, Style::new().dimmed()));
}

/// Build a [`Table`] with `headers`, centralizing the preset choice and
/// its degradation: a boxed UTF-8 grid with a bold header when color is
/// enabled, a frameless ANSI-free layout when piped or `NO_COLOR`.
pub(crate) fn new_table(headers: &[&str]) -> Table {
    let colored = use_color();
    let mut table = Table::new();
    if colored {
        table.load_preset(presets::UTF8_FULL).enforce_styling();
        // `Dynamic` sizes columns to the terminal width. A tty that reports a
        // zero/unknown winsize (CI runners, detached `script` sessions) would
        // otherwise collapse every column to one character, so pin a readable
        // fallback width in that case and let real terminals self-report.
        table.set_content_arrangement(ContentArrangement::Dynamic);
        if terminal_width().is_none() {
            table.set_width(FALLBACK_TABLE_WIDTH);
        }
    } else {
        // Plain, script-safe mode: keep the default `Disabled` arrangement so
        // every row stays on one line for `awk`/`cut`. `Dynamic` here would
        // still wrap to a width `crossterm` can probe via /dev/tty even when
        // stdout is piped, splitting a record across lines.
        table.load_preset(presets::NOTHING);
    }
    let header: Vec<Cell> = headers
        .iter()
        .map(|&h| {
            let cell = Cell::new(h);
            if colored {
                cell.add_attribute(Attribute::Bold)
            } else {
                cell
            }
        })
        .collect();
    table.set_header(header);
    table
}

/// Column width used when the terminal's real width cannot be determined.
const FALLBACK_TABLE_WIDTH: u16 = 120;

/// Current terminal width in columns, or `None` when it cannot be
/// determined or is reported as zero (an unusable winsize).
fn terminal_width() -> Option<u16> {
    match crossterm::terminal::size() {
        Ok((cols, _)) if cols > 0 => Some(cols),
        _ => None,
    }
}

/// Build a STATE cell: green when the worker is alive, red when stale.
/// Color is applied only when [`use_color`] allows it.
pub(crate) fn state_cell(label: &str, ok: bool) -> Cell {
    let cell = Cell::new(label);
    if !use_color() {
        return cell;
    }
    cell.fg(if ok { Color::Green } else { Color::Red })
}

/// Build a KIND cell: blue for app-server services, magenta for API
/// proxies. Color is applied only when [`use_color`] allows it.
pub(crate) fn kind_cell(label: &str, kind: ServiceKind) -> Cell {
    let cell = Cell::new(label);
    if !use_color() {
        return cell;
    }
    cell.fg(match kind {
        ServiceKind::App => Color::Blue,
        ServiceKind::Api => Color::Magenta,
    })
}

/// Render an RFC 3339 timestamp as a coarse "time ago" string. Falls
/// back to the raw input when it cannot be parsed, so a malformed state
/// file never panics the status view.
pub(crate) fn relative_time(rfc3339: &str) -> String {
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return rfc3339.to_string();
    };
    // Clamp to non-negative: a future timestamp (clock drift between hosts,
    // or a hand-edited state.toml) then degrades to "just now" by intent
    // rather than relying on the `< 60` branch swallowing a negative value.
    let secs = (chrono::Utc::now() - parsed.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_returns_raw_string_on_parse_failure() {
        assert_eq!(relative_time("not-a-timestamp"), "not-a-timestamp");
    }

    #[test]
    fn relative_time_buckets_into_human_units() {
        let now = chrono::Utc::now();
        let five_min = (now - chrono::Duration::minutes(5)).to_rfc3339();
        let three_hours = (now - chrono::Duration::hours(3)).to_rfc3339();
        let two_days = (now - chrono::Duration::days(2)).to_rfc3339();

        assert_eq!(relative_time(&now.to_rfc3339()), "just now");
        assert_eq!(relative_time(&five_min), "5m ago");
        assert_eq!(relative_time(&three_hours), "3h ago");
        assert_eq!(relative_time(&two_days), "2d ago");
    }

    #[test]
    fn relative_time_treats_future_timestamps_as_just_now() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        assert_eq!(relative_time(&future), "just now");
    }
}
