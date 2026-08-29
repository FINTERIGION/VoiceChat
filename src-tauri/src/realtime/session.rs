use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    Shutdown,
}

#[derive(Clone)]
pub struct SessionHandle {
    tx: mpsc::UnboundedSender<SessionCommand>,
    /// Mirrors the actor's `is_talking`, so the global hotkey handler (and
    /// anything else outside the Chat tab) can read current mic state
    /// synchronously instead of needing a round trip through the actor.
    mic_open: Arc<AtomicBool>,
}

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
    pub fn interrupt(&self) {
        let _ = self.tx.send(SessionCommand::Interrupt);
    }
    pub fn switch_character(&self, character_id: String) {
        let _ = self.tx.send(SessionCommand::SwitchCharacter(character_id));
    }
    pub fn set_recording(&self, on: bool) {
        let _ = self.tx.send(SessionCommand::SetRecording(on));
    }
    pub fn shutdown(&self) {
        let _ = self.tx.send(SessionCommand::Shutdown);
    }
}

pub fn spawn(app: AppHandle) -> SessionHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let mic_open = Arc::new(AtomicBool::new(false));
    tauri::async_runtime::spawn(run(app, rx, mic_open.clone()));
    SessionHandle { tx, mic_open }
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
    // character's transcript bubbles. There is no backend chat history to
    // reload them from, so without this the old conversation lingers on
    // screen under the new character.
    let _ = app.emit("chat:character", character_id);
}

fn persist_message(app: &AppHandle, conversation_id: &str, role: &str, text: &str) {
    let state = app.state::<AppState>();
    let result = state.db.lock().map_err(|e| e.to_string()).and_then(|conn| {
        message_store::insert_message(&conn, conversation_id, role, text).map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        tracing::error!("failed to persist message: {e}");
    }
}

/// Ends the conversation row and, if it has any content, summarizes it into
/// `memories` (rolling summary + facts). Called on every disconnect, not
/// just clean ones, so a flaky connection still keeps whatever was actually
/// said — it just costs an extra summarization call instead of losing
/// continuity.
async fn finalize_conversation(app: &AppHandle, character_id: &str, character_name: &str, conversation_id: &str) {
    let (messages, previous_summary, api_key, workspace_id, region) = {
        let state = app.state::<AppState>();
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("db lock failed during conversation finalize: {e}");
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
        if let Err(e) = message_store::end_conversation(&conn, conversation_id) {
            tracing::error!("failed to end conversation: {e}");
        }
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
                    | Some(SessionCommand::SetRecording(_)) => {}
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
        emit_recording(&app, memory_enabled.then_some(recording));

        let mut conversation_id: Option<String> = if recording {
            let state = app.state::<AppState>();
            
            match state.db.lock() {
                Ok(conn) => match message_store::start_conversation(&conn, &character_id) {
                    Ok(c) => Some(c.id),
                    Err(e) => {
                        tracing::error!("failed to start conversation: {e}");
                        None
                    }
                },
                Err(e) => {
                    tracing::error!("db lock failed starting conversation: {e}");
                    None
                }
            }
        } else {
            None
        };

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
                    }
                }
                frame = recv_frame(&mut frame_rx) => {
                    if let Some(bytes) = frame {
                        idle_deadline = Instant::now() + IDLE_TIMEOUT;
                        captured_ms += 20;
                        let audio = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        if client::send_event(&mut sink, &ClientEvent::InputAudioBufferAppend { audio }).await.is_err() {
                            break Disconnect::Error("音频发送失败".into());
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
                                        break Disconnect::Error("发送请求失败".into());
                                    }
                                }
                            }
                            if pending_roll && !is_responding {
                                break Disconnect::Rolling;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break Disconnect::Error("连接已断开".into());
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

        if let Some(conv_id) = conversation_id.take() {
            finalize_conversation(&app, &character_id, &character_name, &conv_id).await;
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
    let api_key = secrets::get_api_key().ok_or_else(|| "尚未配置 API Key".to_string())?;
    let (workspace_id, region, current_char, vad_threshold, vad_silence_ms) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let workspace_id = db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?;
        let region = db::get_setting(&conn, "region").map_err(|e| e.to_string())?;
        let char_id = db::get_setting(&conn, "current_character_id")
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "尚未选择角色".to_string())?;
        let character = character::get(&conn, &char_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "角色不存在".to_string())?;
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
                (None, None) => "未知错误".to_string(),
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
