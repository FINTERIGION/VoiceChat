use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep};
use tokio_tungstenite::tungstenite::Message;

use crate::app::state::AppState;
use crate::audio::{capture, playback};
use crate::llm::flash::FlashClient;
use crate::memory;
use crate::prompt::builder::{CharacterPrompt, build_instructions, format_recent_turns};
use crate::secrets;
use crate::store::{character, db, memory as memory_store, message as message_store};
use crate::subtitle;

use super::client::{self, WsSink, WsSource};
use super::events::{ClientEvent, ServerEvent, SessionConfig, TurnDetection};

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
const FALLBACK_VOICE: &str = "longanqian";
/// Hard API context limit (see plan constraint #5): 50 turns / 300s audio.
/// Crossing it mid-conversation requires a summarize-and-reconnect ("会话
/// 滚动"), not just a memory-system nicety.
const ROLL_MAX_TURNS: u32 = 50;
const ROLL_MAX_AUDIO_MS: u64 = 300_000;
/// Fallbacks when the user hasn't set `vad_threshold`/`vad_silence_ms` in
/// Settings yet. Also used by `app::commands::get_vad_settings` so the
/// Settings page shows the same defaults this actually connects with.
pub(crate) const DEFAULT_VAD_THRESHOLD: f32 = 0.5;
pub(crate) const DEFAULT_VAD_SILENCE_MS: u32 = 800;
/// Speech shorter than this while a reply is playing is a backchannel
/// ("嗯"), not an interruption. The line keeps going. Longer than this,
/// local playback is cut and the in-flight response is cancelled.
const BARGE_CONFIRM: Duration = Duration::from_millis(300);
/// After a backchannel, the server may still try to start a reply and
/// answer with "another response is in progress". That is the overlap
/// working as intended, not a failed call.
const OVERLAP_ERROR_HOLD: Duration = Duration::from_millis(1500);

#[derive(Debug)]
pub enum SessionCommand {
    StartTalking,
    StopTalking,
    /// Open if closed, close if open. Used by the global hotkey and the
    /// Chat tab's mic button alike, so both drive the same authoritative
    /// `is_talking` state instead of each guessing independently.
    ToggleTalking,
    Interrupt,
    /// Carries the new current character's id. `voice` only takes effect on
    /// the first `session.update` of a connection, so switching character
    /// always means closing the old WS and opening a fresh one.
    SwitchCharacter(String),
    /// Temporary per-conversation override for "本次不计入记忆" — does not
    /// touch the character's stored `memory_enabled`. Decides only whether
    /// the conversation is summarized into long-term memory; it is saved to
    /// the history either way.
    SetRecording(bool),
    /// Ends the conversation currently being written to (naming and
    /// summarizing it same as any other end) and opens a new one for the
    /// same character. A no-op before any connection has opened one.
    NewConversation,
    Shutdown,
}

#[derive(Clone)]
pub struct SessionHandle {
    tx: mpsc::UnboundedSender<SessionCommand>,
    /// Mirrors the actor's `is_talking`, so the global hotkey handler (and
    /// anything else outside the Chat tab) can read current mic state
    /// synchronously instead of needing a round trip through the actor.
    mic_open: Arc<AtomicBool>,
    /// Mirrors the actor's `conversation_id`: which history row the session
    /// is writing to right now, or `None` between conversations. The
    /// Chat tab reads it to mark that row as live, and
    /// `delete_conversation` reads it to refuse deleting a conversation
    /// still being written to.
    active_conversation: ActiveConversation,
}

/// Shared so the naming task (which outlives the turn that started it) and
/// the Chat tab's `get_active_conversation_id` see the same value the actor
/// last set.
type ActiveConversation = Arc<Mutex<Option<String>>>;

impl SessionHandle {
    pub fn start_talking(&self) {
        let _ = self.tx.send(SessionCommand::StartTalking);
    }
    pub fn stop_talking(&self) {
        let _ = self.tx.send(SessionCommand::StopTalking);
    }
    pub fn toggle_talking(&self) {
        let _ = self.tx.send(SessionCommand::ToggleTalking);
    }
    pub fn mic_open(&self) -> bool {
        self.mic_open.load(Ordering::Acquire)
    }
    pub fn active_conversation(&self) -> Option<String> {
        self.active_conversation
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }
    pub fn interrupt(&self) {
        let _ = self.tx.send(SessionCommand::Interrupt);
    }
    pub fn switch_character(&self, character_id: String) {
        let _ = self.tx.send(SessionCommand::SwitchCharacter(character_id));
    }
    pub fn set_recording(&self, on: bool) {
        let _ = self.tx.send(SessionCommand::SetRecording(on));
    }
    pub fn new_conversation(&self) {
        let _ = self.tx.send(SessionCommand::NewConversation);
    }
    pub fn shutdown(&self) {
        let _ = self.tx.send(SessionCommand::Shutdown);
    }
}

pub fn spawn(app: AppHandle) -> SessionHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let mic_open = Arc::new(AtomicBool::new(false));
    let active_conversation: ActiveConversation = Arc::new(Mutex::new(None));
    tauri::async_runtime::spawn(run(app, rx, mic_open.clone(), active_conversation.clone()));
    SessionHandle {
        tx,
        mic_open,
        active_conversation,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
enum ChatState {
    Idle,
    Connecting,
    Listening,
    Thinking,
    Speaking,
    Error(String),
}

#[derive(Clone, serde::Serialize)]
struct TranscriptEvent {
    role: &'static str,
    text: String,
    done: bool,
}

fn reserve_user_bubble(app: &AppHandle) {
    let _ = app.emit(
        "chat:transcript",
        TranscriptEvent {
            role: "user",
            text: String::new(),
            done: false,
        },
    );
}

fn emit_state(app: &AppHandle, state: ChatState) {
    let _ = app.emit("chat:state", state);
}

/// Lets the subtitle know the current line is over once whatever audio has
/// arrived for it so far has played. Harmless to call more than once for the
/// same line, or for one that ended long ago — the frontend only acts on the
/// first, and only for the line it's showing.
fn finish_subtitle_line(app: &AppHandle, playback: &Option<playback::PlaybackHandle>) {
    if !subtitle::is_open(app) {
        return;
    }
    let id = subtitle::current_line();
    match playback {
        Some(p) => {
            let app = app.clone();
            p.on_drained(move || subtitle::emit_spoken(&app, id));
        }
        None => subtitle::emit_spoken(app, id),
    }
}

/// `None` means the current character has long-term memory switched off
/// altogether, so there is no per-conversation choice to make and the Chat tab
/// hides the toggle entirely. `Some` carries whether *this* conversation will
/// be summarized into long-term memory, which the user can still flip either
/// way. Neither affects the history, which keeps every conversation.
fn emit_recording(app: &AppHandle, on: Option<bool>) {
    let _ = app.emit("chat:recording", on);
}

/// Updates the `SessionHandle::mic_open()` mirror and notifies the frontend.
/// The mirror lets the global hotkey handler read current state
/// synchronously (see `SessionHandle::mic_open`); the event lets any open
/// Chat tab reflect changes made from outside it (the hotkey, or another
/// window).
fn emit_mic(app: &AppHandle, mic_open: &AtomicBool, on: bool) {
    mic_open.store(on, Ordering::Release);
    let _ = app.emit("chat:mic", on);
}

/// Opens or closes the mic — idempotent (no-op if already in the requested
/// state) — so `StartTalking`/`StopTalking`/`ToggleTalking` all funnel
/// through one place that decides what "the mic is open" means, rather than
/// each command reimplementing it and risking drift.
#[allow(clippy::too_many_arguments)]
async fn set_mic_open(
    app: &AppHandle,
    open: bool,
    is_talking: &mut bool,
    is_responding: &mut bool,
    dropping_stale: &mut bool,
    dones_left: &mut u8,
    pending_commit: &mut bool,
    assistant_text: &mut String,
    capture_ctrl: &Option<capture::CaptureControl>,
    playback_handle: &Option<playback::PlaybackHandle>,
    sink: &mut WsSink,
    mic_open: &AtomicBool,
) {
    if open == *is_talking {
        return;
    }
    if open {
        if *is_responding {
            if let Some(p) = playback_handle {
                p.clear();
            }
            let _ = client::send_event(sink, &ClientEvent::ResponseCancel {}).await;
            *is_responding = false;
            *dropping_stale = true;
            *dones_left = 1;
            assistant_text.clear();
        }
        *is_talking = true;
        if let Some(c) = capture_ctrl {
            c.set_capturing(true);
        }
        emit_state(app, ChatState::Listening);
    } else {
        *is_talking = false;
        // A queued commit from a barge-in that hasn't resolved yet must not
        // fire once the mic is closed — there's no live utterance left to
        // submit.
        *pending_commit = false;
        if let Some(c) = capture_ctrl {
            c.set_capturing(false);
        }
    }
    emit_mic(app, mic_open, open);
}

/// Follow-up network action `handle_server_event` wants the caller to take.
/// Kept separate from `handle_server_event` itself (which stays sync and only
/// touches local state) since sending on the WS sink requires `.await`.
enum ServerAction {
    None,
    /// A barge-in was detected (VAD `speech_started` while a response was
    /// in flight) — tell the server to actually stop generating it. Local
    /// playback/state were already cleared by `handle_server_event`.
    CancelResponse,
    /// VAD detected the user stopped talking — submit the turn by
    /// committing the input buffer. Deliberately does *not* also send
    /// `response.create`: this API auto-creates a response itself the
    /// instant its own VAD detects the same speech-stopped boundary
    /// (confirmed by observation — `session.update`'s `create_response:
    /// false` does not appear to suppress it), so a client-sent
    /// `response.create` right after is always a duplicate that loses the
    /// race and gets rejected with "another response is in progress".
    /// `is_responding`/turn accounting instead hang off the first actual
    /// response content event (see `note_turn_started`), which is the only
    /// reliable signal a response really started.
    CommitTurn,
}

/// Commits the buffered input audio. Returns `Err` if the request itself
/// couldn't be sent (caller should treat the connection as dead).
async fn commit_turn(sink: &mut WsSink) -> Result<(), ()> {
    client::send_event(sink, &ClientEvent::InputAudioBufferCommit {})
        .await
        .map_err(|_| ())
}

/// Records that a response has actually started, the first time content for
/// it arrives. This — not the local commit — is what drives `is_responding`
/// and turn/rolling accounting, since the server creates responses on its
/// own schedule (see `ServerAction::CommitTurn`) rather than only in
/// response to a client request.
fn note_turn_started(
    app: &AppHandle,
    is_responding: &mut bool,
    turn_count: &mut u32,
    captured_ms: u64,
    pending_roll: &mut bool,
) {
    *is_responding = true;
    emit_state(app, ChatState::Thinking);
    *turn_count += 1;
    if *turn_count >= ROLL_MAX_TURNS || captured_ms >= ROLL_MAX_AUDIO_MS {
        *pending_roll = true;
    }
}

enum Disconnect {
    /// The user has been idle for too long; do not auto-reconnect.
    Idle,
    /// Actor is shutting down entirely.
    Shutdown,
    /// Unexpected error/close; auto-reconnect with backoff.
    Error(String),
    /// User switched characters; reconnect immediately (no backoff) with the
    /// new one's config.
    SwitchCharacter,
    /// Hit the turn/audio-duration cap; reconnect immediately with the same
    /// character (fresh instructions carry the just-written summary).
    Rolling,
    /// User asked to close out the current conversation and start a fresh
    /// one; reconnect immediately with the same character.
    NewConversation,
}

fn persist_current_character(app: &AppHandle, character_id: &str) {
    let state = app.state::<AppState>();
    let result = state.db.lock().map_err(|e| e.to_string()).and_then(|conn| {
        db::set_setting(&conn, "current_character_id", character_id).map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        tracing::error!("failed to persist current character: {e}");
    }
    // Only called for genuine character switches (never on the initial
    // connect or on an error/rolling reconnect of the same character), so
    // the Chat tab can use this — unlike `chat:state`'s `connecting`, which
    // fires on every reconnect — to know exactly when to drop the previous
    // character's transcript bubbles. The incoming character has no
    // conversation open yet for the tab to replay in their place, so without
    // this the old one lingers on screen under the new character.
    let _ = app.emit("chat:character", character_id);
}

/// Tells any open Chat tab that the history list may have changed, and
/// which of its rows (if any) the live session is writing to right now.
///
/// The active id is read back through `AppState` rather than passed in, so
/// every emitter — including ones several calls deep like `persist_message`
/// — reports the same value the actor last set without having to thread the
/// mirror through.
fn emit_conversations(app: &AppHandle) {
    let active_id = app.state::<AppState>().session.active_conversation();
    let _ = app.emit("chat:conversations", active_id);
}

/// Points the mirror at `id` (or clears it) and notifies the frontend.
fn set_active_conversation(app: &AppHandle, active: &ActiveConversation, id: Option<String>) {
    match active.lock() {
        Ok(mut guard) => *guard = id,
        Err(e) => tracing::error!("active conversation lock poisoned: {e}"),
    }
    emit_conversations(app);
}

fn persist_message(app: &AppHandle, conversation_id: &str, role: &str, text: &str) {
    let state = app.state::<AppState>();
    let result = state.db.lock().map_err(|e| e.to_string()).and_then(|conn| {
        message_store::insert_message(&conn, conversation_id, role, text).map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        tracing::error!("failed to persist message: {e}");
        return;
    }
    // A conversation only enters the history list once it holds something,
    // and its row shows how much was said and what it opened with — all of
    // which just changed.
    emit_conversations(app);
}

/// Gives a conversation its name in the history list.
///
/// A no-op once it has one, so this can run both mid-conversation (as soon
/// as there is a turn to name it after, which is what keeps the newest row
/// from sitting unnamed for the whole session) and again when it ends,
/// without the second call overwriting the first — or overwriting a name
/// the user typed themselves.
async fn name_conversation(app: AppHandle, character_name: String, conversation_id: String) {
    let (messages, api_key, workspace_id, region) = {
        let state = app.state::<AppState>();
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("db lock failed while naming conversation: {e}");
                return;
            }
        };
        // Also false for a conversation that no longer exists (deleted from
        // the history list while it was still being named).
        match message_store::is_untitled(&conn, &conversation_id) {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                tracing::error!("failed to read conversation title: {e}");
                return;
            }
        }
        let messages = match message_store::list_messages(&conn, &conversation_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("failed to list messages for naming: {e}");
                return;
            }
        };
        let workspace_id = db::get_setting(&conn, "workspace_id").ok().flatten();
        let region = db::get_setting(&conn, "region").ok().flatten();
        (messages, secrets::get_api_key(), workspace_id, region)
    };

    let Some(api_key) = api_key else {
        return;
    };
    let client = FlashClient::new(api_key, workspace_id, region.as_deref());
    let title = match memory::generate_title(&client, &character_name, &messages).await {
        Ok(t) => t,
        Err(e) => {
            // Leaves the row labelled by its opening line, which is still
            // readable — not worth failing anything louder over.
            tracing::warn!("conversation naming failed: {e}");
            return;
        }
    };

    let stored = {
        let state = app.state::<AppState>();
        state.db.lock().map_err(|e| e.to_string()).and_then(|conn| {
            message_store::set_title_if_untitled(&conn, &conversation_id, &title)
                .map_err(|e| e.to_string())
        })
    };
    match stored {
        // `false` means it picked up a name while this call was in flight —
        // a hand-typed one, or a concurrent naming attempt. Either way the
        // list already shows it, so there is nothing to announce.
        Ok(true) => emit_conversations(&app),
        Ok(false) => {}
        Err(e) => tracing::error!("failed to store conversation title: {e}"),
    }
}

/// Writes what was said into `memories` (rolling summary + facts), leaving
/// the conversation row itself open, and marks the row as memorized once the
/// summary is stored. Callers decide whether the conversation counts toward
/// memory at all; this only does the writing.
///
/// Split out from `finalize_conversation` because a rolling reconnect needs
/// the summary — it is what the next connection's instructions are built
/// from, the server-side context being exactly what just ran out — but must
/// not end the conversation, which the user is still in the middle of.
async fn summarize_into_memory(
    app: &AppHandle,
    character_id: &str,
    character_name: &str,
    conversation_id: &str,
) {
    let (messages, previous_summary, known_facts, previous_loops, api_key, workspace_id, region) = {
        let state = app.state::<AppState>();
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("db lock failed during conversation summarize: {e}");
                return;
            }
        };
        let messages = message_store::list_messages(&conn, conversation_id).unwrap_or_else(|e| {
            tracing::error!("failed to list messages for summarization: {e}");
            Vec::new()
        });
        let stored = memory_store::list(&conn, character_id).unwrap_or_default();
        let previous_summary = stored
            .iter()
            .find(|m| m.kind == "summary")
            .map(|m| m.content.clone());
        let known_facts = stored
            .iter()
            .filter(|m| m.kind == "fact")
            .cloned()
            .collect::<Vec<_>>();
        let previous_loops = stored
            .iter()
            .filter(|m| m.kind == "open_loop")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>();
        let workspace_id = db::get_setting(&conn, "workspace_id").ok().flatten();
        let region = db::get_setting(&conn, "region").ok().flatten();
        (
            messages,
            previous_summary,
            known_facts,
            previous_loops,
            secrets::get_api_key(),
            workspace_id,
            region,
        )
    };

    if messages.is_empty() {
        return;
    }
    let Some(api_key) = api_key else {
        return;
    };

    let client = FlashClient::new(api_key, workspace_id, region.as_deref());
    let known_fact_texts = known_facts
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>();
    match memory::summarize_conversation(
        &client,
        character_name,
        previous_summary.as_deref(),
        &known_fact_texts,
        &previous_loops,
        &messages,
    )
    .await
    {
        Ok(result) => {
            let stored = {
                let state = app.state::<AppState>();
                state.db.lock().map_err(|e| e.to_string()).and_then(|conn| {
                    memory::store_summary(&conn, character_id, &known_facts, &result)?;
                    message_store::mark_memorized(&conn, conversation_id).map_err(|e| e.to_string())
                })
            };
            match stored {
                // The row's `memorized` just changed, which the history
                // list's delete dialog reads.
                Ok(()) => emit_conversations(app),
                Err(e) => tracing::error!("failed to store memory summary: {e}"),
            }
        }
        Err(e) => tracing::warn!("memory summarization failed: {e}"),
    }
}

/// Ends the conversation row and, if `memorize` and it has any content,
/// summarizes it into `memories`.
///
/// Called when the session is genuinely done writing to this row — not on a
/// dropped-socket or rolling reconnect, which carries on with the same
/// conversation (see `CarriedConversation`). It still runs on unclean ends
/// like an idle timeout, so those keep whatever was actually said.
///
/// `memorize` is the conversation's own choice as it ends — the character's
/// memory switch, narrowed by "本次不计入记忆". A conversation that doesn't
/// count is still named and closed like any other: it stays in the history.
async fn finalize_conversation(
    app: &AppHandle,
    character_id: &str,
    character_name: &str,
    conversation_id: &str,
    memorize: bool,
) {
    // Catches conversations that ended before the mid-session naming pass
    // could run (a single turn, or one where it failed); returns immediately
    // for the rest.
    name_conversation(
        app.clone(),
        character_name.to_string(),
        conversation_id.to_string(),
    )
    .await;

    {
        let state = app.state::<AppState>();
        match state.db.lock() {
            Ok(conn) => {
                if let Err(e) = message_store::end_conversation(&conn, conversation_id) {
                    tracing::error!("failed to end conversation: {e}");
                }
            }
            Err(e) => tracing::error!("db lock failed ending conversation: {e}"),
        }
    }

    if memorize {
        summarize_into_memory(app, character_id, character_name, conversation_id).await;
    }
}

/// A conversation row held open across a reconnect the user never asked
/// for. Carries the character it belongs to, so a connection that came back
/// on a different one appends to a fresh row instead of the wrong history.
struct CarriedConversation {
    id: String,
    character_id: String,
    character_name: String,
    /// The `SetRecording` choice this conversation was left with. "本次不计入
    /// 记忆" is a decision about the conversation, so it has to outlive the
    /// connection that happened to be carrying it.
    recording: bool,
}

/// Opens the history row this connection writes to. Every conversation gets
/// one, whether or not it will count toward long-term memory; `None` only
/// when the database refused.
fn start_conversation_row(app: &AppHandle, character_id: &str) -> Option<String> {
    let state = app.state::<AppState>();
    let conn = match state.db.lock() {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!("db lock failed starting conversation: {e}");
            return None;
        }
    };
    match message_store::start_conversation(&conn, character_id) {
        Ok(c) => Some(c.id),
        Err(e) => {
            tracing::error!("failed to start conversation: {e}");
            None
        }
    }
}

struct Connected {
    sink: WsSink,
    source: WsSource,
    character_id: String,
    character_name: String,
    /// Whether the character allows long-term memory at all. Distinct from
    /// `recording`: this decides whether the choice exists, `recording` is the
    /// choice currently made for this one conversation.
    memory_enabled: bool,
    /// Whether this conversation will be summarized into long-term memory
    /// when it ends. Its transcript goes into the history regardless.
    recording: bool,
    /// Fresh sitting with an unfinished thread: ask the model to say one
    /// sentence once the mic is open, before the user has spoken.
    announce_open_loop: bool,
}

async fn run(
    app: AppHandle,
    mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    mic_open: Arc<AtomicBool>,
    active_conversation: ActiveConversation,
) {
    let (capture_ctrl, mut frame_rx, mut level_rx) = match capture::start() {
        Ok((ctrl, frame_rx, level_rx)) => (Some(ctrl), Some(frame_rx), Some(level_rx)),
        Err(e) => {
            tracing::error!("capture init failed: {e}");
            (None, None, None)
        }
    };
    let playback_handle = match playback::start() {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::error!("playback init failed: {e}");
            None
        }
    };

    let mut reconnect_delay = RECONNECT_MIN;
    let mut auto_reconnect = false;
    // Mic-open intent to resume with on an auto-reconnect. Set to the live
    // `is_talking` when a reconnect is transparent to the user (rolling
    // session cap, transient error), so the mic button doesn't flicker off
    // for reasons the user never triggered. Left `false` for
    // `SwitchCharacter`, which already closes the mic explicitly.
    let mut carried_talking = false;
    // The conversation row a dropped connection was writing to, held across
    // the reconnect so the next one carries on with it. `None` whenever the
    // last disconnect genuinely ended the conversation.
    let mut carried_conversation: Option<CarriedConversation> = None;

    'outer: loop {
        let initial_talking = if auto_reconnect {
            carried_talking
        } else {
            loop {
                match cmd_rx.recv().await {
                    None | Some(SessionCommand::Shutdown) => break 'outer,
                    // Mic is definitely closed before any connection exists,
                    // so toggling can only mean "open".
                    Some(SessionCommand::StartTalking) | Some(SessionCommand::ToggleTalking) => {
                        break true;
                    }
                    Some(SessionCommand::SwitchCharacter(id)) => {
                        persist_current_character(&app, &id);
                        break false;
                    }
                    Some(SessionCommand::StopTalking)
                    | Some(SessionCommand::Interrupt)
                    | Some(SessionCommand::SetRecording(_))
                    | Some(SessionCommand::NewConversation) => {}
                }
            }
        };
        // Carries through connect-failure retries of this same attempt
        // until a `reason` below decides what the *next* attempt should be.
        carried_talking = initial_talking;
        auto_reconnect = false;

        emit_state(&app, ChatState::Connecting);
        let Connected {
            mut sink,
            mut source,
            character_id,
            character_name,
            memory_enabled,
            mut recording,
            mut announce_open_loop,
        } = match connect_with_config(&app, carried_conversation.as_ref()).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("realtime connect failed: {e}");
                emit_state(&app, ChatState::Error(e));
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX);
                auto_reconnect = true;
                continue 'outer;
            }
        };
        reconnect_delay = RECONNECT_MIN;

        let carried = match carried_conversation.take() {
            // Carried into a connection it no longer belongs to — the current
            // character changed while reconnecting. Close the row out rather
            // than leaving it open forever.
            Some(carried) if carried.character_id != character_id => {
                finalize_conversation(
                    &app,
                    &carried.character_id,
                    &carried.character_name,
                    &carried.id,
                    carried.recording,
                )
                .await;
                None
            }
            same_conversation => same_conversation,
        };
        // `recording` arrives here as the character's `memory_enabled`, which
        // keeps the last word — memory switched off in the editor meanwhile
        // takes the resumed conversation out of memory too. Within that, a
        // resumed conversation keeps the choice it was left with: an opt-out
        // that lasted only until the next dropped socket was never really an
        // opt-out.
        if let Some(carried) = &carried {
            recording = recording && carried.recording;
        }
        emit_recording(&app, memory_enabled.then_some(recording));

        let mut conversation_id: Option<String> = match carried {
            // Keep writing to the row the dropped connection opened, so a
            // sitting the user never saw interrupted stays one row in the
            // history list.
            Some(carried) => Some(carried.id),
            None => start_conversation_row(&app, &character_id),
        };
        set_active_conversation(&app, &active_conversation, conversation_id.clone());

        let mut is_talking = initial_talking;
        let mut is_responding = false;
        let mut assistant_text = String::new();
        let mut idle_deadline = Instant::now() + IDLE_TIMEOUT;
        // Set whenever we send `response.cancel`. The server can't stop
        // instantly, so audio/transcript deltas already in flight for the
        // cancelled response keep arriving for a short window; without this
        // gate they'd interleave with the next response (duplicated-looking
        // text, stuttering audio). Cleared when that cancelled response's
        // `response.done` arrives.
        let mut dropping_stale = false;
        // `response.done` events still belonging to a cancelled line.
        // One for a normal interrupt. A backchannel that has to swallow both
        // the original line and the reply the server started for the "嗯"
        // sets this higher.
        let mut dones_left: u8 = 0;
        // Speech started over a reply, and we have not yet decided whether
        // it is a backchannel or an interruption.
        let mut barge_started: Option<Instant> = None;
        let mut overlap_until: Option<Instant> = None;
        let mut opener_grace: Option<Instant> = None;
        let mut heard_user = false;
        // A backchannel arrived while the line was still being generated.
        // When that line finishes, drop the reply the server starts for the
        // "嗯" instead of cancelling the line itself.
        let mut drop_followup = false;
        // Set when `speech_stopped` fires while `dropping_stale` is still
        // true — the user finished a barge-in utterance before the server
        // confirmed cancelling the response it interrupted. Sending
        // `response.create` right away would race the server's own
        // cancellation and get rejected with "another response is in
        // progress"; instead the commit is deferred until that stale
        // response's `response.done` arrives (see `ServerEvent::ResponseDone`).
        let mut pending_commit = false;
        let mut turn_count: u32 = 0;
        let mut captured_ms: u64 = 0;
        let mut pending_roll = false;
        // One naming attempt per connection: the first turn is enough to
        // name a conversation after, and a second attempt would only
        // discover it is already named. A failed attempt isn't retried here
        // either — `finalize_conversation` gets the last word.
        let mut naming_started = false;
        // Diagnostic only: gap between consecutive audio.delta events, to
        // tell server/network delivery jitter apart from local playback
        // issues. Remove once the playback-stutter report is resolved.
        let mut last_delta_at: Option<Instant> = None;

        if let Some(c) = &capture_ctrl {
            c.set_capturing(is_talking);
        }
        emit_state(
            &app,
            if is_talking {
                ChatState::Listening
            } else {
                ChatState::Idle
            },
        );
        emit_mic(&app, &mic_open, is_talking);

        let reason = loop {
            // Once, on a fresh sitting, before any user speech: the
            // instructions already say to raise one unfinished thread.
            if announce_open_loop
                && is_talking
                && !heard_user
                && !is_responding
                && !dropping_stale
                && barge_started.is_none()
            {
                match client::send_event(&mut sink, &ClientEvent::ResponseCreate {}).await {
                    Ok(()) => {
                        announce_open_loop = false;
                        opener_grace = Some(Instant::now() + Duration::from_secs(3));
                        tracing::info!("requesting an open-loop opener");
                    }
                    Err(_) => {
                        break Disconnect::Error(
                            crate::tr!("Failed to send the request", "发送请求失败").into(),
                        );
                    }
                }
            }
            tokio::select! {
                _ = sleep_until_owned(idle_deadline) => {
                    tracing::info!("realtime session idle timeout, closing");
                    let _ = sink.close().await;
                    break Disconnect::Idle;
                }
                _ = barge_confirm_sleep(barge_started), if barge_started.is_some() => {
                    barge_started = None;
                    drop_followup = false;
                    tracing::info!("barge-in confirmed");
                    if let Some(p) = &playback_handle {
                        p.clear();
                    }
                    if is_responding {
                        let _ = client::send_event(&mut sink, &ClientEvent::ResponseCancel {}).await;
                        is_responding = false;
                        dropping_stale = true;
                        dones_left = 1;
                        assistant_text.clear();
                    }
                    last_delta_at = None;
                    emit_state(
                        &app,
                        if is_talking { ChatState::Listening } else { ChatState::Idle },
                    );
                }
                cmd = cmd_rx.recv() => {
                    idle_deadline = Instant::now() + IDLE_TIMEOUT;
                    tracing::info!(?cmd, is_talking, is_responding, "session command received");
                    match cmd {
                        None | Some(SessionCommand::Shutdown) => {
                            if let Some(c) = &capture_ctrl { c.set_capturing(false); }
                            let _ = sink.close().await;
                            break Disconnect::Shutdown;
                        }
                        // `StopTalking` doesn't submit a turn on its own — that
                        // happens automatically as the user pauses (see
                        // `ServerAction::CommitTurn` below), so closing the
                        // mic here doesn't risk submitting a mid-sentence,
                        // unfinished one.
                        Some(SessionCommand::StartTalking) => {
                            barge_started = None;
                            drop_followup = false;
                            set_mic_open(&app, true, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut dones_left, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::StopTalking) => {
                            barge_started = None;
                            drop_followup = false;
                            set_mic_open(&app, false, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut dones_left, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::ToggleTalking) => {
                            barge_started = None;
                            drop_followup = false;
                            set_mic_open(&app, !is_talking, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut dones_left, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::Interrupt) => {
                            barge_started = None;
                            drop_followup = false;
                            if is_responding {
                                if let Some(p) = &playback_handle { p.clear(); }
                                let _ = client::send_event(&mut sink, &ClientEvent::ResponseCancel {}).await;
                                is_responding = false;
                                dropping_stale = true;
                                dones_left = 1;
                                assistant_text.clear();
                            }
                        }
                        Some(SessionCommand::SwitchCharacter(id)) => {
                            if let Some(c) = &capture_ctrl { c.set_capturing(false); }
                            if let Some(p) = &playback_handle { p.clear(); }
                            emit_mic(&app, &mic_open, false);
                            persist_current_character(&app, &id);
                            let _ = sink.close().await;
                            break Disconnect::SwitchCharacter;
                        }
                        Some(SessionCommand::SetRecording(on)) => {
                            // The Chat tab hides the toggle when the character
                            // has memory off, so this should be unreachable —
                            // but a stray command must never switch memory on
                            // for a character that opted out of it. Only what
                            // happens at the end changes: the transcript keeps
                            // going into the history either way.
                            if memory_enabled {
                                recording = on;
                                emit_recording(&app, Some(recording));
                            }
                        }
                        Some(SessionCommand::NewConversation) => {
                            if let Some(p) = &playback_handle { p.clear(); }
                            let _ = sink.close().await;
                            break Disconnect::NewConversation;
                        }
                    }
                }
                frame = recv_frame(&mut frame_rx) => {
                    if let Some(bytes) = frame {
                        idle_deadline = Instant::now() + IDLE_TIMEOUT;
                        captured_ms += 20;
                        let audio = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        if client::send_event(&mut sink, &ClientEvent::InputAudioBufferAppend { audio }).await.is_err() {
                            break Disconnect::Error(crate::tr!("Failed to send audio", "音频发送失败").into());
                        }
                    }
                }
                level = recv_level(&mut level_rx) => {
                    if let Some(v) = level {
                        let _ = app.emit("chat:level", v);
                    }
                }
                msg = source.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            idle_deadline = Instant::now() + IDLE_TIMEOUT;
                            let action = handle_server_event(
                                &app,
                                text.as_str(),
                                &playback_handle,
                                &mut assistant_text,
                                &mut is_responding,
                                &mut dropping_stale,
                                &mut dones_left,
                                &mut barge_started,
                                &mut overlap_until,
                                &opener_grace,
                                &mut heard_user,
                                &mut drop_followup,
                                &mut pending_commit,
                                conversation_id.as_deref(),
                                &mut last_delta_at,
                                is_talking,
                                &mut turn_count,
                                captured_ms,
                                &mut pending_roll,
                            );
                            match action {
                                ServerAction::None => {}
                                ServerAction::CancelResponse => {
                                    let _ = client::send_event(&mut sink, &ClientEvent::ResponseCancel {}).await;
                                }
                                ServerAction::CommitTurn => {
                                    tracing::info!("vad speech stopped: sending input_audio_buffer.commit");
                                    if commit_turn(&mut sink).await.is_err() {
                                        break Disconnect::Error(crate::tr!("Failed to send the request", "发送请求失败").into());
                                    }
                                }
                            }
                            // As soon as one turn has been said and
                            // answered there is enough to name the
                            // conversation after — waiting until it ends
                            // would leave the history list's newest row
                            // labelled only by its opening line for the
                            // whole session.
                            if !naming_started && !is_responding && turn_count >= 1 {
                                if let Some(id) = &conversation_id {
                                    naming_started = true;
                                    tauri::async_runtime::spawn(name_conversation(
                                        app.clone(),
                                        character_name.clone(),
                                        id.clone(),
                                    ));
                                }
                            }
                            if pending_roll && !is_responding {
                                break Disconnect::Rolling;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break Disconnect::Error(crate::tr!("Connection closed", "连接已断开").into());
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            break Disconnect::Error(e.to_string());
                        }
                    }
                }
            }
        };

        if let Some(c) = &capture_ctrl {
            c.set_capturing(false);
        }
        // A reply the connection dropped in the middle of never gets its
        // `response.done`, so this is the last chance to end its line.
        finish_subtitle_line(&app, &playback_handle);

        // A reconnect nobody asked for — a dropped socket, or the turn/audio
        // cap rolling the session — is invisible to the user, who is still in
        // the same conversation. Ending the row here is what used to split one
        // sitting into a string of short conversations, plus (the server drops
        // the socket after a few minutes of silence) a pile of empty ones.
        // Every other reason really has ended it.
        let carry_conversation = matches!(&reason, Disconnect::Error(_) | Disconnect::Rolling);

        if let Some(conv_id) = conversation_id.take() {
            if carry_conversation {
                // Rolling means the server-side context ran out, so the next
                // connection's instructions have to carry the conversation
                // instead. The row itself stays open, and stays live. One
                // that doesn't count toward memory carries on through its
                // recent lines alone (see `connect_with_config`).
                if matches!(&reason, Disconnect::Rolling) && recording {
                    summarize_into_memory(&app, &character_id, &character_name, &conv_id).await;
                }
                carried_conversation = Some(CarriedConversation {
                    id: conv_id,
                    character_id: character_id.clone(),
                    character_name: character_name.clone(),
                    recording,
                });
            } else if matches!(&reason, Disconnect::SwitchCharacter) {
                // Editing the live character reconnects at once. Summarizing
                // the conversation just ended is a model call (16.9s in the
                // run that froze the start button) and the actor cannot open
                // the mic while it awaits that call. The summary still lands
                // in long-term memory; this sitting just doesn't wait for it.
                set_active_conversation(&app, &active_conversation, None);
                let app = app.clone();
                let character_id = character_id.clone();
                let character_name = character_name.clone();
                tauri::async_runtime::spawn(async move {
                    finalize_conversation(&app, &character_id, &character_name, &conv_id, recording)
                        .await;
                });
            } else {
                // The session has stopped writing to this conversation, so the
                // Chat tab should stop showing its row as live now rather than
                // after the naming and summarization round trips below.
                set_active_conversation(&app, &active_conversation, None);
                finalize_conversation(&app, &character_id, &character_name, &conv_id, recording)
                    .await;
            }
        }

        match reason {
            Disconnect::Shutdown => break 'outer,
            Disconnect::Idle => {
                emit_state(&app, ChatState::Idle);
            }
            Disconnect::Error(e) => {
                emit_state(&app, ChatState::Error(e));
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX);
                auto_reconnect = true;
                carried_talking = is_talking;
            }
            Disconnect::SwitchCharacter => {
                auto_reconnect = true;
                carried_talking = false;
            }
            Disconnect::Rolling => {
                tracing::info!("rolling session: turn/duration cap reached, reconnecting");
                auto_reconnect = true;
                carried_talking = is_talking;
            }
            Disconnect::NewConversation => {
                auto_reconnect = true;
                carried_talking = is_talking;
            }
        }
    }
}

async fn sleep_until_owned(deadline: Instant) {
    tokio::time::sleep_until(deadline).await;
}

async fn barge_confirm_sleep(started: Option<Instant>) {
    match started {
        Some(t) => tokio::time::sleep_until(t + BARGE_CONFIRM).await,
        None => std::future::pending().await,
    }
}

fn is_overlap_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("in progress")
        || lower.contains("another response")
        || message.contains("正在进行")
        || message.contains("进行中")
}

async fn recv_frame(rx: &mut Option<capture::FrameReceiver>) -> Option<Vec<u8>> {
    match rx {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_level(rx: &mut Option<capture::LevelReceiver>) -> Option<f32> {
    match rx {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}

async fn connect_with_config(
    app: &AppHandle,
    carried: Option<&CarriedConversation>,
) -> Result<Connected, String> {
    let api_key = secrets::get_api_key()
        .ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API Key").to_string())?;
    let (workspace_id, region, current_char, vad_threshold, vad_silence_ms) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let workspace_id = db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?;
        let region = db::get_setting(&conn, "region").map_err(|e| e.to_string())?;
        let char_id = db::get_setting(&conn, "current_character_id")
            .map_err(|e| e.to_string())?
            .ok_or_else(|| crate::tr!("No character selected", "尚未选择角色").to_string())?;
        let character = character::get(&conn, &char_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| crate::tr!("Character not found", "角色不存在").to_string())?;
        // Hands-free voice detection is a user preference (mic/environment
        // dependent), not a character trait, so it lives in the global
        // `settings` table rather than on the character row.
        let vad_threshold = db::get_setting(&conn, "vad_threshold")
            .map_err(|e| e.to_string())?
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(DEFAULT_VAD_THRESHOLD);
        let vad_silence_ms = db::get_setting(&conn, "vad_silence_ms")
            .map_err(|e| e.to_string())?
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_VAD_SILENCE_MS);
        (
            workspace_id,
            region,
            character,
            vad_threshold,
            vad_silence_ms,
        )
    };

    // A dropped socket or a rolled context is still the same sitting. The
    // new connection's server memory is empty, so the last few lines have
    // to ride along in the instructions — memory or not, since they are what
    // the model was holding a moment ago rather than anything remembered from
    // an earlier conversation. A different character is a different sitting,
    // so those lines stay out.
    let continuing_id =
        carried.and_then(|c| (c.character_id == current_char.id).then(|| c.id.clone()));

    let (memories, recent_turns) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let memories = if current_char.memory_enabled {
            memory::select_for_injection(&conn, &current_char.id)?
        } else {
            Vec::new()
        };
        let recent_turns = match continuing_id.as_deref() {
            Some(id) => {
                let messages = message_store::list_messages(&conn, id).unwrap_or_else(|e| {
                    tracing::error!("failed to list messages for resume: {e}");
                    Vec::new()
                });
                let pairs: Vec<(&str, &str)> = messages
                    .iter()
                    .map(|m| (m.role.as_str(), m.text.as_str()))
                    .collect();
                format_recent_turns(&current_char.name, &pairs)
            }
            None => None,
        };
        (memories, recent_turns)
    };
    let has_open_loop = memories
        .iter()
        .any(|m| m.kind == "open_loop" && !m.content.trim().is_empty());
    let memory_block = if memories.is_empty() {
        None
    } else {
        let flash = FlashClient::new(api_key.clone(), workspace_id.clone(), region.as_deref());
        memory::build_injection_block(&flash, &memories).await
    };
    let invite_open_loop = continuing_id.is_none()
        && has_open_loop
        && memory_block.as_ref().is_some_and(|m| !m.trim().is_empty());

    let url = client::realtime_url(workspace_id.as_deref(), region.as_deref());
    let (mut sink, source) = client::connect(&url, &api_key).await?;

    let instructions = build_instructions(&CharacterPrompt {
        name: &current_char.name,
        persona: &current_char.persona,
        language: &current_char.language,
        speech_habits: &current_char.speech_habits,
        memory_block: memory_block.as_deref(),
        recent_turns: recent_turns.as_deref(),
        invite_open_loop,
    });

    let turn_detection = Some(TurnDetection::ServerVad {
        threshold: vad_threshold,
        silence_duration_ms: vad_silence_ms,
        create_response: false,
    });

    let session_update = ClientEvent::SessionUpdate {
        session: SessionConfig {
            modalities: vec!["text".into(), "audio".into()],
            voice: current_char
                .voice_id
                .clone()
                .unwrap_or_else(|| FALLBACK_VOICE.into()),
            instructions,
            input_audio_format: "pcm".into(),
            output_audio_format: "pcm".into(),
            turn_detection,
            max_history_turns: current_char.max_history_turns.max(1) as u32,
        },
    };
    client::send_event(&mut sink, &session_update).await?;

    Ok(Connected {
        sink,
        source,
        character_id: current_char.id,
        character_name: current_char.name,
        memory_enabled: current_char.memory_enabled,
        recording: current_char.memory_enabled,
        announce_open_loop: invite_open_loop,
    })
}

fn event_kind(event: &ServerEvent) -> &'static str {
    match event {
        ServerEvent::SessionCreated {} => "session.created",
        ServerEvent::SpeechStarted {} => "speech_started",
        ServerEvent::SpeechStopped {} => "speech_stopped",
        ServerEvent::ResponseAudioDelta { .. } => "audio.delta",
        ServerEvent::ResponseAudioTranscriptDelta { .. } => "transcript.delta",
        ServerEvent::ResponseAudioTranscriptDone { .. } => "transcript.done",
        ServerEvent::InputAudioTranscriptionCompleted { .. } => "input_transcription.completed",
        ServerEvent::ResponseDone {} => "response.done",
        ServerEvent::Error { .. } => "error",
        ServerEvent::Unknown => "unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_server_event(
    app: &AppHandle,
    text: &str,
    playback: &Option<playback::PlaybackHandle>,
    assistant_text: &mut String,
    is_responding: &mut bool,
    dropping_stale: &mut bool,
    dones_left: &mut u8,
    barge_started: &mut Option<Instant>,
    overlap_until: &mut Option<Instant>,
    opener_grace: &Option<Instant>,
    heard_user: &mut bool,
    drop_followup: &mut bool,
    pending_commit: &mut bool,
    conversation_id: Option<&str>,
    last_delta_at: &mut Option<Instant>,
    is_talking: bool,
    turn_count: &mut u32,
    captured_ms: u64,
    pending_roll: &mut bool,
) -> ServerAction {
    let event: ServerEvent = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to parse realtime server event: {e}");
            return ServerAction::None;
        }
    };

    tracing::debug!(
        event = event_kind(&event),
        dropping_stale = *dropping_stale,
        is_responding = *is_responding,
        "server event"
    );

    match event {
        ServerEvent::SessionCreated {} => {
            tracing::info!("realtime session created");
            ServerAction::None
        }
        ServerEvent::SpeechStarted {} => {
            *heard_user = true;
            // Don't cut on the first sound. A short "嗯" should leave the
            // line playing; only speech that lasts past `BARGE_CONFIRM`
            // takes the floor. An explicit interrupt or opening the mic
            // still cuts immediately — those aren't this event.
            if *is_responding && barge_started.is_none() && !*dropping_stale {
                *barge_started = Some(Instant::now());
                tracing::info!("speech over a reply; waiting before cutting");
                return ServerAction::None;
            }
            // A gate left up for a backchannel reply must not swallow the
            // next thing the user actually says.
            if !*is_responding && *dropping_stale && barge_started.is_none() {
                *dropping_stale = false;
                *dones_left = 0;
                *overlap_until = Some(Instant::now() + OVERLAP_ERROR_HOLD);
                *last_delta_at = None;
                emit_state(app, ChatState::Listening);
                return ServerAction::CancelResponse;
            }
            *last_delta_at = None;
            if !*is_responding {
                emit_state(app, ChatState::Listening);
            }
            ServerAction::None
        }
        ServerEvent::SpeechStopped {} => {
            if let Some(started) = barge_started.take() {
                if started.elapsed() < BARGE_CONFIRM {
                    *overlap_until = Some(Instant::now() + OVERLAP_ERROR_HOLD);
                    tracing::info!("backchannel; keeping the current line");
                    if !*is_responding {
                        // The server already ended the line on its own. This
                        // speech_stopped would start a reply to the "嗯".
                        // Cancel that, and leave audio already buffered playing.
                        *dropping_stale = true;
                        *dones_left = 1;
                        return ServerAction::CancelResponse;
                    }
                    *drop_followup = true;
                    return ServerAction::None;
                }
                tracing::info!("barge-in confirmed at speech stop");
                *drop_followup = false;
                if *is_responding {
                    if let Some(p) = playback {
                        p.clear();
                    }
                    *is_responding = false;
                    *dropping_stale = true;
                    *dones_left = 1;
                    assistant_text.clear();
                    *last_delta_at = None;
                    if is_talking {
                        reserve_user_bubble(app);
                        *pending_commit = true;
                    }
                    emit_state(
                        app,
                        if is_talking {
                            ChatState::Listening
                        } else {
                            ChatState::Idle
                        },
                    );
                    return ServerAction::CancelResponse;
                }
                if let Some(p) = playback {
                    p.clear();
                }
            }
            // The only turn boundary there is — the user paused, submit
            // what they said. Guarded on `is_talking` in case this arrives
            // just after the mic was manually closed.
            if is_talking && !*is_responding {
                // The server transcribes the just-committed audio
                // asynchronously, in parallel with generating the reply —
                // its `conversation.item.input_audio_transcription.completed`
                // can arrive well after the assistant's transcript deltas,
                // even after `response.done`. Reserve the user bubble's
                // position now, at the one point we know for certain a turn
                // boundary occurred, so the UI doesn't show the reply above
                // the message that prompted it. Filled in once the real
                // transcript event arrives (see `InputAudioTranscriptionCompleted`).
                reserve_user_bubble(app);
                if *dropping_stale {
                    // A barge-in just interrupted the previous response and
                    // this utterance ended before the server confirmed
                    // cancelling it (its `response.done` hasn't arrived
                    // yet). Committing now risks the server trying to
                    // auto-create the next response while the old one is
                    // still in progress — defer it instead.
                    *pending_commit = true;
                    ServerAction::None
                } else {
                    ServerAction::CommitTurn
                }
            } else {
                ServerAction::None
            }
        }
        ServerEvent::ResponseAudioDelta { delta } => {
            // The original line already ended during the confirm window, so
            // this delta is a new reply (usually to a backchannel). Drop it
            // until that reply's own `response.done`.
            if barge_started.is_some() && !*is_responding {
                *dropping_stale = true;
                if *dones_left == 0 {
                    *dones_left = 1;
                }
                return ServerAction::None;
            }
            if *dropping_stale {
                return ServerAction::None;
            }
            if !*is_responding {
                note_turn_started(app, is_responding, turn_count, captured_ms, pending_roll);
            }
            let now = Instant::now();
            if let Some(prev) = last_delta_at.replace(now) {
                let gap_ms = now.duration_since(prev).as_millis();
                if gap_ms >= 100 {
                    tracing::info!(gap_ms, "large gap between audio.delta events");
                }
            }
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&delta) {
                if let Some(p) = playback {
                    p.append(bytes);
                }
            }
            emit_state(app, ChatState::Speaking);
            ServerAction::None
        }
        ServerEvent::ResponseAudioTranscriptDelta { delta } => {
            if barge_started.is_some() && !*is_responding {
                *dropping_stale = true;
                if *dones_left == 0 {
                    *dones_left = 1;
                }
                return ServerAction::None;
            }
            if *dropping_stale {
                return ServerAction::None;
            }
            if !*is_responding {
                note_turn_started(app, is_responding, turn_count, captured_ms, pending_roll);
            }
            // Checked once and reused below rather than asking twice: cheap
            // either way, but there's no reason to look the window up more
            // than once per delta.
            let subtitle_open = subtitle::is_open(app);
            if subtitle_open && assistant_text.is_empty() {
                subtitle::begin_line();
            }
            assistant_text.push_str(&delta);
            let _ = app.emit(
                "chat:transcript",
                TranscriptEvent {
                    role: "assistant",
                    text: assistant_text.clone(),
                    done: false,
                },
            );
            if subtitle_open {
                let line_id = subtitle::current_line();
                subtitle::emit_line(app, line_id, assistant_text.as_str(), false);
                subtitle::translate_line(app, line_id, assistant_text.as_str(), false);
            }
            ServerAction::None
        }
        ServerEvent::ResponseAudioTranscriptDone { transcript } => {
            if *dropping_stale {
                return ServerAction::None;
            }
            let text = transcript.unwrap_or_else(|| assistant_text.clone());
            let _ = app.emit(
                "chat:transcript",
                TranscriptEvent {
                    role: "assistant",
                    text: text.clone(),
                    done: true,
                },
            );
            if subtitle::is_open(app) {
                let line_id = subtitle::current_line();
                subtitle::emit_line(app, line_id, &text, true);
                subtitle::translate_line(app, line_id, &text, true);
            }
            assistant_text.clear();
            if let Some(conv_id) = conversation_id {
                persist_message(app, conv_id, "assistant", &text);
            }
            ServerAction::None
        }
        ServerEvent::InputAudioTranscriptionCompleted { transcript } => {
            let _ = app.emit(
                "chat:transcript",
                TranscriptEvent {
                    role: "user",
                    text: transcript.clone(),
                    done: true,
                },
            );
            if let Some(conv_id) = conversation_id {
                persist_message(app, conv_id, "user", &transcript);
            }
            ServerAction::None
        }
        ServerEvent::ResponseDone {} => {
            // Every branch below ends some response, whether it's the line
            // on screen finishing normally, or one cut off by a barge-in
            // (its audio already cleared, so this fires right away).
            finish_subtitle_line(app, playback);
            if *drop_followup && !*dropping_stale {
                *drop_followup = false;
                *is_responding = false;
                *last_delta_at = None;
                *dropping_stale = true;
                *dones_left = 1;
                emit_state(
                    app,
                    if is_talking {
                        ChatState::Listening
                    } else {
                        ChatState::Idle
                    },
                );
                return ServerAction::None;
            }
            if *dropping_stale {
                // This was a cancelled response's own done event. A
                // backchannel may still be waiting on a second one — the
                // reply the server started for the overlap — and clearing
                // the gate on the first would let that reply play.
                if *dones_left > 1 {
                    *dones_left -= 1;
                    return ServerAction::None;
                }
                *dones_left = 0;
                *dropping_stale = false;
                if *pending_commit {
                    // A barge-in utterance finished while we were still
                    // waiting on this — the cancellation is now confirmed,
                    // so it's safe to submit the queued turn. Still guarded
                    // on `is_talking` in case the mic was closed meanwhile
                    // (which also clears `pending_commit`, but belt and
                    // braces since state can only be read, not assumed,
                    // this far from where it changed).
                    *pending_commit = false;
                    if is_talking {
                        return ServerAction::CommitTurn;
                    }
                }
                return ServerAction::None;
            }
            *is_responding = false;
            *last_delta_at = None;
            // In VAD mode the mic stays open across turns, so if it's still
            // open go back to "listening for the next thing you say" rather
            // than "idle" (which reads as the conversation having ended).
            emit_state(
                app,
                if is_talking {
                    ChatState::Listening
                } else {
                    ChatState::Idle
                },
            );
            ServerAction::None
        }
        ServerEvent::Error { error } => {
            tracing::error!("realtime error event: {error:?}");
            let message = match (error.code, error.message) {
                (Some(code), Some(msg)) => format!("[{code}] {msg}"),
                (None, Some(msg)) => msg,
                (Some(code), None) => code,
                (None, None) => crate::tr!("Unknown error", "未知错误").to_string(),
            };
            if opener_grace.is_some_and(|until| Instant::now() < until) {
                tracing::warn!("ignored error during open-loop opener: {message}");
                return ServerAction::None;
            }
            if overlap_until.is_some_and(|until| Instant::now() < until)
                && is_overlap_error(&message)
            {
                tracing::info!("ignored turn-overlap error: {message}");
                return ServerAction::None;
            }
            emit_state(app, ChatState::Error(message));
            ServerAction::None
        }
        ServerEvent::Unknown => {
            tracing::debug!("unknown realtime event ignored: {text}");
            ServerAction::None
        }
    }
}
