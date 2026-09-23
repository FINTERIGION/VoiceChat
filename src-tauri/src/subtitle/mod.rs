//! The desktop subtitle: a borderless, click-through, always-on-top window
//! mirroring the assistant's live transcript, with an optional line
//! translating it into the app's display language underneath.
//!
//! Kept as its own module rather than folded into `realtime::session`: it
//! owns a window plus two settings and a translation call, none of which the
//! session actor otherwise needs beyond the couple of call sites in
//! `session::handle_server_event`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::app::state::AppState;
use crate::i18n::{self, Lang};
use crate::llm::flash::FlashClient;
use crate::secrets;
use crate::store::{character, db};

/// Window label. `main.tsx` reads this back (via `getCurrentWindow().label`)
/// to decide whether to render the subtitle UI or the regular app.
pub const LABEL: &str = "subtitle";

const DEFAULT_WIDTH: f64 = 680.0;
const DEFAULT_HEIGHT: f64 = 150.0;
/// Gap kept above the taskbar so the subtitle doesn't sit flush against it.
const BOTTOM_MARGIN: f64 = 90.0;

/// Bumped once per assistant turn (see `begin_line`). `subtitle:line` events
/// carry the current value, and `spawn_translation` carries it along with
/// the request, so a translation that resolves after the line it was for
/// has already been replaced can be told apart from one that still matters.
static LINE_ID: AtomicU64 = AtomicU64::new(0);

/// How far `translate_line` has got through the current line. A line is sent
/// off for translation a sentence at a time while it's still streaming in,
/// rather than whole once the turn is over — which left the translation
/// trailing the original by the entire reply plus a round trip.
struct Progress {
    line: u64,
    /// The leading part of the line already sent off, as sent.
    sent: String,
    /// How many segments that was, i.e. the next segment's index.
    segments: u32,
}

static PROGRESS: Mutex<Progress> = Mutex::new(Progress {
    line: 0,
    sent: String::new(),
    segments: 0,
});

/// Shorter than this, a finished sentence waits to go along with the next
/// one: a lone "嗯。" or "Okay." isn't worth a request of its own, and
/// translates worse with nothing around it.
const MIN_SEGMENT_CHARS: usize = 4;
/// A run this long with no sentence end yet gets cut at a comma instead, so
/// one long sentence doesn't hold its whole translation back until it ends.
const SOFT_CUT_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SubtitleSettings {
    pub enabled: bool,
    pub translate: bool,
}

pub fn get_settings(conn: &Connection) -> rusqlite::Result<SubtitleSettings> {
    Ok(SubtitleSettings {
        enabled: db::get_setting(conn, "subtitle_enabled")?.as_deref() == Some("1"),
        translate: db::get_setting(conn, "subtitle_translate")?.as_deref() == Some("1"),
    })
}

pub fn set_settings(conn: &Connection, settings: SubtitleSettings) -> rusqlite::Result<()> {
    db::set_setting(
        conn,
        "subtitle_enabled",
        if settings.enabled { "1" } else { "0" },
    )?;
    db::set_setting(
        conn,
        "subtitle_translate",
        if settings.translate { "1" } else { "0" },
    )?;
    Ok(())
}

fn saved_position(conn: &Connection) -> rusqlite::Result<Option<(f64, f64)>> {
    let x = db::get_setting(conn, "subtitle_x")?.and_then(|s| s.parse().ok());
    let y = db::get_setting(conn, "subtitle_y")?.and_then(|s| s.parse().ok());
    Ok(x.zip(y))
}

pub fn save_position(conn: &Connection, x: f64, y: f64) -> rusqlite::Result<()> {
    db::set_setting(conn, "subtitle_x", &x.to_string())?;
    db::set_setting(conn, "subtitle_y", &y.to_string())?;
    Ok(())
}

/// Bottom-center of the primary monitor, in logical pixels — used the first
/// time the window ever opens, before the user has dragged it anywhere.
fn default_position(app: &AppHandle) -> (f64, f64) {
    match app.primary_monitor() {
        Ok(Some(monitor)) => {
            let scale = monitor.scale_factor();
            let size = monitor.size();
            let w = size.width as f64 / scale;
            let h = size.height as f64 / scale;
            (
                ((w - DEFAULT_WIDTH) / 2.0).max(0.0),
                (h - DEFAULT_HEIGHT - BOTTOM_MARGIN).max(0.0),
            )
        }
        // No monitor info (headless/CI, or the call failed) — still has to
        // land somewhere on screen rather than error out of opening at all.
        _ => (200.0, 600.0),
    }
}

pub fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).is_some()
}

/// Idempotent — a no-op if the window is already open. Called both at
/// startup (see `lib.rs`'s `setup`, when the setting was already on) and
/// from `app::commands::set_subtitle_settings` when the user just switched
/// it on.
///
/// Must be called from an `async` command or from `setup` (before the event
/// loop is running), never from a synchronous command: `WebviewWindowBuilder
/// ::build` deadlocks on Windows when invoked from the thread commands
/// normally run on (documented on the builder itself).
pub fn open(app: &AppHandle) -> Result<(), String> {
    if is_open(app) {
        return Ok(());
    }

    let saved = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        saved_position(&conn).map_err(|e| e.to_string())?
    };
    let (x, y) = saved.unwrap_or_else(|| default_position(app));

    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Subtitle")
        .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;

    // Click-through by default, so the subtitle never eats a click meant for
    // whatever's underneath it; `set_adjusting` lifts this only while the
    // user is dragging it into place.
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn close(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Toggles "调整字幕位置": click-through off (so the window can be dragged)
/// while `on`, and saves wherever it ended up once switched back off. A
/// no-op if the subtitle window isn't open — nothing to adjust.
pub fn set_adjusting(app: &AppHandle, on: bool) -> Result<(), String> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    window
        .set_ignore_cursor_events(!on)
        .map_err(|e| e.to_string())?;
    if !on {
        let physical = window.outer_position().map_err(|e| e.to_string())?;
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        save_position(&conn, physical.x as f64 / scale, physical.y as f64 / scale)
            .map_err(|e| e.to_string())?;
    }
    let _ = app.emit("subtitle:adjust", on);
    Ok(())
}

/// Starts a new subtitle line and returns its id. Called once per assistant
/// turn, right as its first transcript delta arrives — not any earlier, so a
/// turn nothing was ever said for doesn't bump the counter.
pub fn begin_line() -> u64 {
    LINE_ID.fetch_add(1, Ordering::Relaxed) + 1
}

/// The id of the line currently on screen (or most recently was).
pub fn current_line() -> u64 {
    LINE_ID.load(Ordering::Relaxed)
}

#[derive(Clone, Serialize)]
struct LineEvent {
    id: u64,
    text: String,
    done: bool,
}

pub fn emit_line(app: &AppHandle, id: u64, text: &str, done: bool) {
    let _ = app.emit(
        "subtitle:line",
        LineEvent {
            id,
            text: text.to_string(),
            done,
        },
    );
}

/// Line `id` has finished playing out loud — or was cut off, or never will
/// finish now. The frontend starts its fade-out countdown from this rather
/// than from the line's `done`, which only means the text is complete: for a
/// long reply that lands many seconds before the audio does.
pub fn emit_spoken(app: &AppHandle, id: u64) {
    let _ = app.emit("subtitle:spoken", id);
}

/// One segment of a line's translation. Segments are requested concurrently
/// and can resolve out of order; `index` is where this one goes.
#[derive(Clone, Serialize)]
struct TranslationEvent {
    id: u64,
    index: u32,
    text: String,
}

struct TranslationContext {
    character_language: String,
    api_key: Option<String>,
    workspace_id: Option<String>,
    region: Option<String>,
}

/// `None` covers every reason there is nothing to translate: the setting is
/// off, no character is selected, or it no longer exists.
fn translation_context(conn: &Connection) -> rusqlite::Result<Option<TranslationContext>> {
    if !get_settings(conn)?.translate {
        return Ok(None);
    }
    let Some(char_id) = db::get_setting(conn, "current_character_id")? else {
        return Ok(None);
    };
    let Some(character) = character::get(conn, &char_id)? else {
        return Ok(None);
    };
    Ok(Some(TranslationContext {
        character_language: character.language,
        api_key: secrets::get_api_key(),
        workspace_id: db::get_setting(conn, "workspace_id")?,
        region: db::get_setting(conn, "region")?,
    }))
}

/// The character is already speaking the display language, so a translation
/// would just repeat it. `auto` (the character follows whoever it's talking
/// to) still asks for one — there's no way to know here whether that happens
/// to be the display language too — and the frontend hides the line itself
/// if the two turn out identical.
fn should_translate(character_language: &str, target: Lang) -> bool {
    !matches!(
        (character_language, target),
        ("zh", Lang::Zh) | ("en", Lang::En)
    )
}

fn target_language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "English",
        Lang::Zh => "Simplified Chinese",
    }
}

/// Sentence-ending punctuation: a segment can end right after one of these.
fn is_sentence_end(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '；' | '…' | '.' | '!' | '?')
}

/// Clause-level punctuation, only cut at once a run has gone on for
/// `SOFT_CUT_CHARS` without a sentence end.
fn is_clause_end(c: char) -> bool {
    matches!(c, '，' | '、' | '：' | ',' | ':')
}

/// Closing quotes and brackets, kept with the sentence they close.
fn is_closer(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '”' | '’' | '」' | '』' | '）' | ')' | '】' | '》'
    )
}

/// Byte offset into `tail` just past the last place it can be cut, if any.
///
/// Only cuts once something has already come after the punctuation — until
/// then there's no telling "好。" from "好。」", or "3." from "3.14". ASCII
/// punctuation additionally needs whitespace after it, since mid-word it's as
/// likely a decimal point, an abbreviation, or a thousands separator.
fn segment_end(tail: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = tail.char_indices().collect();
    let mut end = None;
    let mut run = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        run += 1;
        if !is_sentence_end(c) && !(run >= SOFT_CUT_CHARS && is_clause_end(c)) {
            i += 1;
            continue;
        }
        // Carry on through any further punctuation or closing quotes ("？！",
        // "。」") so they stay with the sentence they belong to.
        let mut j = i + 1;
        while j < chars.len() && (is_sentence_end(chars[j].1) || is_closer(chars[j].1)) {
            j += 1;
        }
        let Some(&(next_at, next)) = chars.get(j) else {
            break;
        };
        if !c.is_ascii() || next.is_whitespace() {
            end = Some(next_at);
            run = 0;
        } else {
            run += j - i - 1;
        }
        i = j;
    }
    end
}

/// Called with the line so far on every transcript delta (`done: false`),
/// and once more with the final text (`done: true`). Sends off each complete
/// sentence for translation as soon as it's there, and on `done` whatever's
/// left, so the first sentence's translation can be on screen while later
/// ones are still being spoken.
pub fn translate_line(app: &AppHandle, id: u64, text: &str, done: bool) {
    let (index, earlier, segment) = {
        let mut progress = PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
        if progress.line != id {
            *progress = Progress {
                line: id,
                sent: String::new(),
                segments: 0,
            };
        }
        let Some(tail) = text.strip_prefix(progress.sent.as_str()) else {
            // Only possible if the final transcript disagrees with the
            // deltas it was streamed as. What's been sent is still right as
            // far as it goes; there's just no telling where the rest starts.
            tracing::warn!("subtitle translation: final transcript diverged from its deltas");
            return;
        };
        let end = if done {
            tail.len()
        } else {
            match segment_end(tail) {
                Some(end) => end,
                None => return,
            }
        };
        let segment = tail[..end].trim();
        if segment.is_empty() || (!done && segment.chars().count() < MIN_SEGMENT_CHARS) {
            return;
        }
        let segment = segment.to_string();
        let earlier = progress.sent.trim().to_string();
        let index = progress.segments;
        progress.sent.push_str(&tail[..end]);
        progress.segments += 1;
        (index, earlier, segment)
    };
    spawn_translation(app, id, index, earlier, segment);
}

/// Fire-and-forget: translates `text` (segment `index` of subtitle line `id`)
/// into the display language and emits `subtitle:translation` once it
/// resolves. `earlier` is the part of the line before it, for the model to
/// resolve references against. Silent on failure or when there's nothing to
/// do — the subtitle simply stays single-language, a fine degradation for
/// something this secondary.
fn spawn_translation(app: &AppHandle, id: u64, index: u32, earlier: String, text: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let context = {
            let state = app.state::<AppState>();
            let conn = match state.db.lock() {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::error!("subtitle translation: db lock poisoned: {e}");
                    return;
                }
            };
            match translation_context(&conn) {
                Ok(Some(ctx)) => ctx,
                Ok(None) => return,
                Err(e) => {
                    tracing::error!("subtitle translation: failed to read settings: {e}");
                    return;
                }
            }
        };

        let target = i18n::current();
        if !should_translate(&context.character_language, target) {
            return;
        }
        let Some(api_key) = context.api_key else {
            return;
        };

        let flash = FlashClient::new(api_key, context.workspace_id, context.region.as_deref());
        let mut system = format!(
            "Translate the subtitle line the user sends into {}. Output only the \
             translation itself, with no quotes, labels, or explanation.",
            target_language_name(target)
        );
        if !earlier.is_empty() {
            system.push_str(&format!(
                "\n\nThe line continues this earlier part of the same reply, which \
                 has already been translated separately. Use it only to resolve \
                 references; do not translate or repeat it.\n<earlier>{earlier}</earlier>"
            ));
        }
        let started = Instant::now();
        match flash.complete_fast(&system, &text).await {
            Ok(translated) => {
                tracing::debug!(
                    index,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "subtitle segment translated"
                );
                let translated = translated.trim();
                // Segments are concatenated as-is on screen, so English needs
                // its sentence gap put back; Chinese runs on without one.
                let text = if index > 0 && matches!(target, Lang::En) {
                    format!(" {translated}")
                } else {
                    translated.to_string()
                };
                let _ = app.emit("subtitle:translation", TranslationEvent { id, index, text });
            }
            Err(e) => {
                tracing::warn!("subtitle translation failed: {e}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(tail: &str) -> Option<&str> {
        segment_end(tail).map(|end| &tail[..end])
    }

    #[test]
    fn cuts_after_the_last_finished_sentence() {
        assert_eq!(cut("你好。今天天气不错！我们"), Some("你好。今天天气不错！"));
        assert_eq!(cut("Hello there. How are you? I"), Some("Hello there. How are you?"));
    }

    /// Nothing after the punctuation yet, so it might still be followed by
    /// a closing quote, or turn out to be a decimal point.
    #[test]
    fn waits_for_what_follows_the_punctuation() {
        assert_eq!(cut("你好。"), None);
        assert_eq!(cut("It costs 3."), None);
        assert_eq!(cut("It costs 3.14 dollars"), None);
    }

    #[test]
    fn keeps_closing_quotes_and_stacked_punctuation_with_their_sentence() {
        assert_eq!(cut("他说：「好。」然后"), Some("他说：「好。」"));
        assert_eq!(cut("真的吗？！我不信"), Some("真的吗？！"));
        assert_eq!(cut("She said \"hi.\" Then"), Some("She said \"hi.\""));
    }

    #[test]
    fn cuts_long_runs_at_a_comma_but_not_short_ones() {
        assert_eq!(cut("短句，接着"), None);
        let long = format!("{}，接着", "长".repeat(SOFT_CUT_CHARS));
        let expected = format!("{}，", "长".repeat(SOFT_CUT_CHARS));
        assert_eq!(cut(&long), Some(expected.as_str()));
        assert_eq!(cut(&format!("{} 1,000", "x".repeat(SOFT_CUT_CHARS))), None);
    }
}
