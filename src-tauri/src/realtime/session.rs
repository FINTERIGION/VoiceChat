use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

use crate::app::state::AppState;
use crate::audio::{capture, playback};
use crate::llm::flash::FlashClient;
use crate::memory;
use crate::prompt::builder::{build_instructions, CharacterPrompt};
use crate::secrets;
use crate::store::{character, db, memory as memory_store, message as message_store};

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
    /// Temporary per-conversation override for "本次对话不记录" — does not
    /// touch the character's stored `memory_enabled`.
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
    /// is writing to right now, or `None` when it isn't recording one. The
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
    tauri::async_runtime::spawn(run(
        app,
        rx,
        mic_open.clone(),
        active_conversation.clone(),
    ));
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

fn emit_state(app: &AppHandle, state: ChatState) {
    let _ = app.emit("chat:state", state);
}

/// `None` means the current character has long-term memory switched off
/// altogether, so there is no per-conversation choice to make and the Chat tab
/// hides the toggle entirely. `Some` carries whether *this* conversation is
/// being recorded, which the user can still flip either way.
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
/// the conversation row itself open.
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
    let (messages, previous_summary, api_key, workspace_id, region) = {
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
        let previous_summary = memory_store::list(&conn, character_id)
            .ok()
            .and_then(|mems| mems.into_iter().find(|m| m.kind == "summary"))
            .map(|m| m.content);
        let workspace_id = db::get_setting(&conn, "workspace_id").ok().flatten();
        let region = db::get_setting(&conn, "region").ok().flatten();
        (messages, previous_summary, secrets::get_api_key(), workspace_id, region)
    };

    if messages.is_empty() {
        return;
    }
    let Some(api_key) = api_key else {
        return;
    };

    let client = FlashClient::new(api_key, workspace_id, region.as_deref());
    match memory::summarize_conversation(&client, character_name, previous_summary.as_deref(), &messages).await {
        Ok(result) => {
            let state = app.state::<AppState>();
            match state.db.lock() {
                Ok(conn) => {
                    if let Err(e) = memory::store_summary(&conn, character_id, &result) {
                        tracing::error!("failed to store memory summary: {e}");
                    }
                }
                Err(e) => tracing::error!("db lock failed while storing memory: {e}"),
            };
        }
        Err(e) => tracing::warn!("memory summarization failed: {e}"),
    }
}

/// Ends the conversation row and, if it has any content, summarizes it into
/// `memories`.
///
/// Called when the session is genuinely done writing to this row — not on a
/// dropped-socket or rolling reconnect, which carries on with the same
/// conversation (see `CarriedConversation`). It still runs on unclean ends
/// like an idle timeout, so those keep whatever was actually said.
async fn finalize_conversation(app: &AppHandle, character_id: &str, character_name: &str, conversation_id: &str) {
    // Catches conversations that ended before the mid-session naming pass
    // could run (a single turn, or one where it failed); returns immediately
    // for the rest.
    name_conversation(app.clone(), character_name.to_string(), conversation_id.to_string()).await;

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

    summarize_into_memory(app, character_id, character_name, conversation_id).await;
}

/// A conversation row held open across a reconnect the user never asked
/// for. Carries the character it belongs to, so a connection that came back
/// on a different one appends to a fresh row instead of the wrong history.
struct CarriedConversation {
    id: String,
    character_id: String,
    character_name: String,
    /// The `SetRecording` choice this conversation was left with. "本次对话
    /// 不记录" is a decision about the conversation, so it has to outlive the
    /// connection that happened to be carrying it.
    recording: bool,
}

/// Opens the history row this connection writes to, or `None` when nothing
/// is being recorded.
fn start_conversation_row(app: &AppHandle, character_id: &str, recording: bool) -> Option<String> {
    if !recording {
        return None;
    }
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
    recording: bool,
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
                        break true
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
        } = match connect_with_config(&app).await {
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
            // character changed while reconnecting, or memory was switched
            // off for this one in the editor meanwhile. Close the row out
            // rather than leaving it open forever.
            Some(carried) if carried.character_id != character_id || !memory_enabled => {
                finalize_conversation(
                    &app,
                    &carried.character_id,
                    &carried.character_name,
                    &carried.id,
                )
                .await;
                None
            }
            same_conversation => same_conversation,
        };
        // `recording` arrives here as the character's `memory_enabled`, which
        // keeps the last word — but within that, a resumed conversation keeps
        // the choice it was left with. An opt-out that lasted only until the
        // next dropped socket was never really an opt-out.
        if let Some(carried) = &carried {
            recording = recording && carried.recording;
        }
        emit_recording(&app, memory_enabled.then_some(recording));

        let mut conversation_id: Option<String> = match carried {
            // Keep writing to the row the dropped connection opened, so a
            // sitting the user never saw interrupted stays one row in the
            // history list. Kept even while recording is paused: the row is
            // what a later `SetRecording(true)` goes back to writing into.
            Some(carried) => Some(carried.id),
            None => start_conversation_row(&app, &character_id, recording),
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
            tokio::select! {
                _ = sleep_until_owned(idle_deadline) => {
                    tracing::info!("realtime session idle timeout, closing");
                    let _ = sink.close().await;
                    break Disconnect::Idle;
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
                            set_mic_open(&app, true, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::StopTalking) => {
                            set_mic_open(&app, false, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::ToggleTalking) => {
                            set_mic_open(&app, !is_talking, &mut is_talking, &mut is_responding, &mut dropping_stale, &mut pending_commit, &mut assistant_text, &capture_ctrl, &playback_handle, &mut sink, &mic_open).await;
                        }
                        Some(SessionCommand::Interrupt) => {
                            if is_responding {
                                if let Some(p) = &playback_handle { p.clear(); }
                                let _ = client::send_event(&mut sink, &ClientEvent::ResponseCancel {}).await;
                                is_responding = false;
                                dropping_stale = true;
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
                            // but a stray command must never switch recording
                            // on for a character that opted out of memory.
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
                                &mut pending_commit,
                                conversation_id.as_deref(),
                                recording,
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
                // instead. The row itself stays open, and stays live.
                if matches!(&reason, Disconnect::Rolling) {
                    summarize_into_memory(&app, &character_id, &character_name, &conv_id).await;
                }
                carried_conversation = Some(CarriedConversation {
                    id: conv_id,
                    character_id: character_id.clone(),
                    character_name: character_name.clone(),
                    recording,
                });
            } else {
                // The session has stopped writing to this conversation, so the
                // Chat tab should stop showing its row as live now rather than
                // after the naming and summarization round trips below.
                set_active_conversation(&app, &active_conversation, None);
                finalize_conversation(&app, &character_id, &character_name, &conv_id).await;
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

async fn connect_with_config(app: &AppHandle) -> Result<Connected, String> {
    let api_key = secrets::get_api_key().ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API Key").to_string())?;
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
        (workspace_id, region, character, vad_threshold, vad_silence_ms)
    };

    let memories = if current_char.memory_enabled {
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        memory::top_k_memories(&conn, &current_char.id)?
    } else {
        Vec::new()
    };
    let memory_block = if memories.is_empty() {
        None
    } else {
        let flash = FlashClient::new(api_key.clone(), workspace_id.clone(), region.as_deref());
        memory::build_injection_block(&flash, &memories).await
    };

    let url = client::realtime_url(workspace_id.as_deref(), region.as_deref());
    let (mut sink, source) = client::connect(&url, &api_key).await?;

    let instructions = build_instructions(&CharacterPrompt {
        name: &current_char.name,
        persona: &current_char.persona,
        language: &current_char.language,
        speech_habits: &current_char.speech_habits,
        memory_block: memory_block.as_deref(),
    });

    let turn_detection = Some(TurnDetection::ServerVad {
        threshold: vad_threshold,
        silence_duration_ms: vad_silence_ms,
        create_response: false,
    });

    let session_update = ClientEvent::SessionUpdate {
        session: SessionConfig {
            modalities: vec!["text".into(), "audio".into()],
            voice: current_char.voice_id.clone().unwrap_or_else(|| FALLBACK_VOICE.into()),
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
    pending_commit: &mut bool,
    conversation_id: Option<&str>,
    recording: bool,
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
            // Barge-in: the user started talking while a response was in
            // flight. Silence locally right away — don't wait on the
            // network round trip to the server's own cancel ack.
            let action = if *is_responding {
                if let Some(p) = playback {
                    p.clear();
                }
                *is_responding = false;
                *dropping_stale = true;
                assistant_text.clear();
                ServerAction::CancelResponse
            } else {
                ServerAction::None
            };
            *last_delta_at = None;
            emit_state(app, ChatState::Listening);
            action
        }
        ServerEvent::SpeechStopped {} => {
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
                let _ = app.emit(
                    "chat:transcript",
                    TranscriptEvent {
                        role: "user",
                        text: String::new(),
                        done: false,
                    },
                );
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
            if *dropping_stale {
                return ServerAction::None;
            }
            if !*is_responding {
                note_turn_started(app, is_responding, turn_count, captured_ms, pending_roll);
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
            assistant_text.clear();
            if recording {
                if let Some(conv_id) = conversation_id {
                    persist_message(app, conv_id, "assistant", &text);
                }
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
            if recording {
                if let Some(conv_id) = conversation_id {
                    persist_message(app, conv_id, "user", &transcript);
                }
            }
            ServerAction::None
        }
        ServerEvent::ResponseDone {} => {
            if *dropping_stale {
                // This was the cancelled response's own done event.
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
                if is_talking { ChatState::Listening } else { ChatState::Idle },
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
            emit_state(app, ChatState::Error(message));
            ServerAction::None
        }
        ServerEvent::Unknown => {
            tracing::debug!("unknown realtime event ignored: {text}");
            ServerAction::None
        }
    }
}
