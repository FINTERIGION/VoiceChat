use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::app::state::AppState;
use crate::i18n::{self, Lang};
use crate::llm::flash::FlashClient;
use crate::secrets::{self, SecretStatus};
use crate::store::{backup, character, db, memory, message};
use crate::voice::{self, service::VoiceService};

#[derive(Serialize, Deserialize, Default)]
pub struct ConnectionSettings {
    pub workspace_id: Option<String>,
    pub region: Option<String>,
}

#[tauri::command]
pub fn get_secret_status() -> SecretStatus {
    secrets::status()
}

#[tauri::command]
pub fn set_api_key(key: String) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(crate::tr!("API key cannot be empty", "API key 不能为空").into());
    }
    secrets::set_api_key(key)
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    secrets::clear_api_key()
}

#[tauri::command]
pub fn list_regions() -> Vec<crate::dashscope::RegionOption> {
    crate::dashscope::regions()
}

/// The language the UI is displayed in, as a BCP-47-ish tag (`en` /
/// `zh-CN`). Unset means the user has never chosen, which is the default.
#[tauri::command]
pub fn get_ui_language(state: State<AppState>) -> Result<String, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let stored = db::get_setting(&conn, "ui_language").map_err(|e| e.to_string())?;
    Ok(match stored {
        Some(tag) if !tag.is_empty() => Lang::from_tag(&tag).tag().to_string(),
        _ => i18n::DEFAULT.tag().to_string(),
    })
}

/// Persists the choice *and* applies it to this process, so backend-produced
/// strings (command errors, session error states, region labels) switch over
/// with the rest of the UI instead of only after a restart.
#[tauri::command]
pub fn set_ui_language(state: State<AppState>, language: String) -> Result<(), String> {
    let lang = Lang::from_tag(&language);
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::set_setting(&conn, "ui_language", lang.tag()).map_err(|e| e.to_string())?;
    i18n::set(lang);
    Ok(())
}

#[tauri::command]
pub fn get_connection_settings(state: State<AppState>) -> Result<ConnectionSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(ConnectionSettings {
        workspace_id: db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
        region: db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
    })
}

/// Both values end up interpolated into the API hostname every request is
/// sent to, with the API key on it — so they are checked here, at the one
/// place they enter the app, rather than trusted because the UI happens to
/// offer a dropdown for one and a short text box for the other. A pasted
/// `evil.com/#` as the workspace id would otherwise be a hostname the key
/// gets handed to; see `dashscope::is_valid_workspace_id`.
#[tauri::command]
pub fn set_connection_settings(
    state: State<AppState>,
    settings: ConnectionSettings,
) -> Result<(), String> {
    let workspace_id = settings.workspace_id.as_deref().unwrap_or("").trim();
    if !workspace_id.is_empty() && !crate::dashscope::is_valid_workspace_id(workspace_id) {
        return Err(crate::tr!(
            "That doesn't look like a Workspace ID — enter the id from the Model Studio console (letters, digits, - and _ only), not a URL",
            "这不像是 Workspace ID。请填写百炼控制台中的工作空间 ID（只含字母、数字、- 和 _），而不是网址",
        )
        .into());
    }

    let region = settings.region.as_deref().unwrap_or("").trim();
    if !region.is_empty() && !crate::dashscope::is_valid_region(region) {
        return Err(crate::tr!(
            format!("Unknown region {region:?}"),
            format!("未知的地域 {region:?}"),
        ));
    }

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::set_setting(&conn, "workspace_id", workspace_id).map_err(|e| e.to_string())?;
    db::set_setting(&conn, "region", region).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct VadSettings {
    pub threshold: f64,
    pub silence_ms: i64,
}

#[tauri::command]
pub fn get_vad_settings(state: State<AppState>) -> Result<VadSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let threshold = db::get_setting(&conn, "vad_threshold")
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(crate::realtime::session::DEFAULT_VAD_THRESHOLD as f64);
    let silence_ms = db::get_setting(&conn, "vad_silence_ms")
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(crate::realtime::session::DEFAULT_VAD_SILENCE_MS as i64);
    Ok(VadSettings { threshold, silence_ms })
}

#[tauri::command]
pub fn set_vad_settings(state: State<AppState>, settings: VadSettings) -> Result<(), String> {
    let current_id = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "vad_threshold", &settings.threshold.to_string())
            .map_err(|e| e.to_string())?;
        db::set_setting(&conn, "vad_silence_ms", &settings.silence_ms.to_string())
            .map_err(|e| e.to_string())?;
        db::get_setting(&conn, "current_character_id").map_err(|e| e.to_string())?
    };
    // Reconnect so an already-open session picks up the new threshold/
    // silence duration immediately instead of waiting for the next natural
    // reconnect (idle timeout, error retry, character switch).
    if let Some(id) = current_id.filter(|s| !s.is_empty()) {
        state.session.switch_character(id);
    }
    Ok(())
}

#[tauri::command]
pub async fn test_connectivity(state: State<'_, AppState>) -> Result<(), String> {
    let api_key = secrets::get_api_key()
        .ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API key"))?;
    let (workspace_id, region) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        (
            db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
            db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
        )
    };
    FlashClient::new(api_key, workspace_id, region.as_deref())
        .test_connectivity()
        .await
}

#[tauri::command]
pub fn start_talking(state: State<AppState>) {
    state.session.start_talking();
}

#[tauri::command]
pub fn stop_talking(state: State<AppState>) {
    state.session.stop_talking();
}

#[tauri::command]
pub fn toggle_talking(state: State<AppState>) {
    state.session.toggle_talking();
}

#[tauri::command]
pub fn get_mic_open(state: State<AppState>) -> bool {
    state.session.mic_open()
}

/// `None` means the hotkey is disabled; collapses the DB's "never set" vs
/// "explicitly disabled" distinction into the actual default it's running
/// with, since the frontend only needs to know what's active right now.
#[tauri::command]
pub fn get_hotkey(state: State<AppState>) -> Result<Option<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let stored = db::get_setting(&conn, "hotkey").map_err(|e| e.to_string())?;
    Ok(match stored {
        None => Some(crate::DEFAULT_HOTKEY.to_string()),
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s),
    })
}

/// `accelerator: None` (or empty) disables the hotkey. Registers the new
/// combo before unregistering the old one, so an invalid/taken combo leaves
/// the previous one working and reports the error instead of leaving the
/// user with nothing.
///
/// Separate from the command because restoring a backup applies a hotkey the
/// same way — including the part where the combo may already belong to some
/// other app on this machine.
fn apply_hotkey(
    app: &AppHandle,
    state: &AppState,
    accelerator: Option<String>,
) -> Result<(), String> {
    let previous_stored = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::get_setting(&conn, "hotkey").map_err(|e| e.to_string())?
    };
    let previous_active = match previous_stored {
        None => Some(crate::DEFAULT_HOTKEY.to_string()),
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s),
    };

    let new_value = accelerator.filter(|s| !s.trim().is_empty());
    if new_value == previous_active {
        return Ok(());
    }

    if let Some(accel) = &new_value {
        crate::install_hotkey(app, accel)?;
    }
    if let Some(old) = &previous_active {
        let _ = app.global_shortcut().unregister(old.as_str());
    }

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::set_setting(&conn, "hotkey", new_value.as_deref().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_hotkey(
    app: AppHandle,
    state: State<AppState>,
    accelerator: Option<String>,
) -> Result<(), String> {
    apply_hotkey(&app, &state, accelerator)
}

#[tauri::command]
pub fn interrupt(state: State<AppState>) {
    state.session.interrupt();
}

#[tauri::command]
pub fn set_recording(state: State<AppState>, on: bool) {
    state.session.set_recording(on);
}

// ---- Memories ----

#[tauri::command]
pub fn list_memories(
    state: State<AppState>,
    character_id: String,
) -> Result<Vec<memory::Memory>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::list(&conn, &character_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_memory(
    state: State<AppState>,
    id: String,
    content: String,
) -> Result<memory::Memory, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::update_content(&conn, &id, &content)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| crate::tr!("Memory not found", "记忆不存在").to_string())
}

#[tauri::command]
pub fn delete_memory(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::delete(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_memories(state: State<AppState>, ids: Vec<String>) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::delete_many(&mut conn, &ids).map_err(|e| e.to_string())
}

// ---- Conversations ----

#[tauri::command]
pub fn list_conversations(
    state: State<AppState>,
    character_id: String,
) -> Result<Vec<message::ConversationSummary>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    message::list_conversations(&conn, &character_id).map_err(|e| e.to_string())
}

/// The transcript of one past conversation, oldest message first.
#[tauri::command]
pub fn get_conversation_messages(
    state: State<AppState>,
    id: String,
) -> Result<Vec<message::Message>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    message::list_messages(&conn, &id).map_err(|e| e.to_string())
}

/// Which conversation the live session is writing to, for a Chat tab that
/// mounted after the `chat:conversations` event announcing it.
#[tauri::command]
pub fn get_active_conversation_id(state: State<AppState>) -> Option<String> {
    state.session.active_conversation()
}

/// A hand-typed name always wins: the automatic naming pass skips any
/// conversation that already has one, so this is never overwritten.
#[tauri::command]
pub fn rename_conversation(
    state: State<AppState>,
    id: String,
    title: String,
) -> Result<message::ConversationSummary, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(crate::tr!("The name cannot be empty", "名称不能为空").into());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    message::set_title(&conn, &id, title)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| crate::tr!("Conversation not found", "会话不存在").to_string())
}

/// Ends the conversation the live session is writing to (named and
/// summarized into memory same as any other end) and starts a fresh one for
/// the same character. A no-op if nothing is currently being recorded.
#[tauri::command]
pub fn new_conversation(state: State<AppState>) {
    state.session.new_conversation();
}

#[tauri::command]
pub fn delete_conversation(state: State<AppState>, id: String) -> Result<(), String> {
    // The live session holds this id and keeps inserting messages against
    // it; deleting the row out from under it would make every following
    // turn fail its foreign key, and the conversation would reappear as a
    // half-written row on the next refresh.
    if state.session.active_conversation().as_deref() == Some(id.as_str()) {
        return Err(crate::tr!(
            "This conversation is still going — close the mic and let it end before deleting it",
            "这是正在进行中的会话，请先结束后再删除",
        )
        .into());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    message::delete_conversation(&conn, &id).map_err(|e| e.to_string())
}

// ---- Characters ----

#[tauri::command]
pub fn list_characters(state: State<AppState>) -> Result<Vec<character::Character>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    character::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_character(
    state: State<AppState>,
    input: character::CharacterInput,
) -> Result<character::Character, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    character::create(&conn, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_character(
    state: State<AppState>,
    id: String,
    input: character::CharacterInput,
) -> Result<character::Character, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let updated = character::update(&conn, &id, input)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| crate::tr!("Character not found", "角色不存在").to_string())?;

    // `voice`/`instructions` only take effect on a connection's first
    // `session.update`, so if we just edited the character that's currently
    // live, the running WS won't pick up the change (new language, persona,
    // voice, etc.) until something else reconnects it. Force that now.
    let is_current = db::get_setting(&conn, "current_character_id")
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some(id.as_str());
    if is_current {
        state.session.switch_character(id);
    }

    Ok(updated)
}

#[tauri::command]
pub fn delete_character(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let current = db::get_setting(&conn, "current_character_id").map_err(|e| e.to_string())?;
    character::delete(&conn, &id).map_err(|e| e.to_string())?;

    if current.as_deref() == Some(id.as_str()) {
        let remaining = character::list(&conn).map_err(|e| e.to_string())?;
        match remaining.first() {
            Some(next) => {
                db::set_setting(&conn, "current_character_id", &next.id)
                    .map_err(|e| e.to_string())?;
                state.session.switch_character(next.id.clone());
            }
            None => {
                db::set_setting(&conn, "current_character_id", "").map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_current_character_id(state: State<AppState>) -> Result<Option<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let id = db::get_setting(&conn, "current_character_id").map_err(|e| e.to_string())?;
    Ok(id.filter(|s| !s.is_empty()))
}

#[tauri::command]
pub fn switch_character(state: State<AppState>, id: String) {
    state.session.switch_character(id);
}

#[derive(Serialize)]
pub struct PersonaPolish {
    pub persona: String,
    pub speech_habits: String,
}

#[tauri::command]
pub async fn polish_persona(
    state: State<'_, AppState>,
    description: String,
) -> Result<PersonaPolish, String> {
    let api_key = secrets::get_api_key()
        .ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API key"))?;
    let (workspace_id, region) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        (
            db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
            db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
        )
    };
    let client = FlashClient::new(api_key, workspace_id, region.as_deref());
    let raw = client
        .complete(
            "你是角色设定助手。根据用户的一句话描述，扩写出适合语音陪聊角色的人设。\
             严格只输出 JSON，不要有任何其他文字，格式为 \
             {\"persona\": \"2-3 句的人设描述\", \"speech_habits\": \"1-2 句的语言习惯描述\"}",
            &description,
        )
        .await?;

    #[derive(Deserialize)]
    struct Parsed {
        persona: String,
        speech_habits: String,
    }
    match serde_json::from_str::<Parsed>(raw.trim()) {
        Ok(p) => Ok(PersonaPolish {
            persona: p.persona,
            speech_habits: p.speech_habits,
        }),
        Err(e) => {
            tracing::warn!("failed to parse persona polish response as JSON: {e}; raw: {raw}");
            Ok(PersonaPolish {
                persona: raw,
                speech_habits: String::new(),
            })
        }
    }
}

// ---- Voices ----

fn voice_service(state: &AppState) -> Result<VoiceService, String> {
    let api_key = secrets::get_api_key()
        .ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API Key"))?;
    let (workspace_id, region) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        (
            db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
            db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
        )
    };
    Ok(VoiceService::new(api_key, workspace_id, region.as_deref()))
}

#[tauri::command]
pub fn list_preset_voices() -> Vec<&'static str> {
    voice::service::PRESET_VOICES.to_vec()
}

#[tauri::command]
pub fn start_recording(state: State<AppState>) -> Result<(), String> {
    let mut guard = state.recorder.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err(crate::tr!("Already recording", "已经在录音").into());
    }
    *guard = Some(voice::clone::start()?);
    Ok(())
}

#[tauri::command]
pub fn stop_recording(state: State<AppState>) -> Result<String, String> {
    let handle = state
        .recorder
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .ok_or_else(|| crate::tr!("Recording has not been started", "尚未开始录音"))?;
    handle.stop_and_encode()
}

#[tauri::command]
pub async fn clone_voice(
    state: State<'_, AppState>,
    prefix: String,
    audio_url: String,
) -> Result<String, String> {
    voice_service(&state)?
        .clone_voice(voice::service::REALTIME_TARGET_MODEL, &prefix, &audio_url)
        .await
}

#[derive(Serialize)]
pub struct DesignPreviewResult {
    pub tts_voice: Option<String>,
    pub preview_audio_data_uri: String,
}

#[tauri::command]
pub async fn design_voice_preview(
    state: State<'_, AppState>,
    voice_prompt: String,
    preview_text: String,
    prefix: String,
) -> Result<DesignPreviewResult, String> {
    let preview = voice_service(&state)?
        .design_voice(&voice_prompt, &preview_text, &prefix)
        .await?;
    Ok(DesignPreviewResult {
        tts_voice: preview.tts_voice,
        preview_audio_data_uri: format!("data:audio/wav;base64,{}", preview.preview_audio_b64),
    })
}

#[tauri::command]
pub fn slugify(input: String) -> String {
    voice::slugify(&input)
}

#[derive(Serialize)]
pub struct ManagedVoice {
    pub voice_id: String,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub bound_character_id: Option<String>,
    pub bound_character_name: Option<String>,
    /// Whether the live session can speak with this voice. False for the
    /// TTS-series voices an account also collects (the Voice Design flow
    /// leaves one behind each time it runs).
    pub realtime_compatible: bool,
}

#[tauri::command]
pub async fn list_voices(state: State<'_, AppState>) -> Result<Vec<ManagedVoice>, String> {
    let entries = voice_service(&state)?.list_voices().await?;
    let characters = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        character::list(&conn).map_err(|e| e.to_string())?
    };
    Ok(entries
        .into_iter()
        .map(|v| {
            let bound = characters
                .iter()
                .find(|c| c.voice_id.as_deref() == Some(v.voice_id.as_str()));
            ManagedVoice {
                realtime_compatible: v.is_realtime(),
                voice_id: v.voice_id,
                status: v.status,
                created_at: v.gmt_create,
                bound_character_id: bound.map(|c| c.id.clone()),
                bound_character_name: bound.map(|c| c.name.clone()),
            }
        })
        .collect())
}

#[tauri::command]
pub async fn delete_voice(state: State<'_, AppState>, voice_id: String) -> Result<(), String> {
    let bound_name = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        character::list(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| c.voice_id.as_deref() == Some(voice_id.as_str()))
            .map(|c| c.name)
    };
    if let Some(name) = bound_name {
        return Err(crate::tr!(
            format!(
                "This voice is still bound to character \"{name}\" — switch that character to a different voice before deleting it"
            ),
            format!("音色仍绑定在角色「{name}」上，请先在该角色里更换音色再删除"),
        ));
    }
    voice_service(&state)?.delete_voice(&voice_id).await
}

// ---- Backup ----

/// A file dialog parented to the app window and filtered to backup files.
///
/// Parenting matters on Windows: an unparented dialog can end up behind the
/// window that opened it, looking like the app has frozen.
fn backup_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut builder = app
        .dialog()
        .file()
        .add_filter(crate::tr!("VoiceChat backup", "VoiceChat 备份"), &["json"]);
    if let Some(window) = app.get_webview_window("main") {
        builder = builder.set_parent(&window);
    }
    builder
}

/// The native dialogs answer through a callback; every caller here wants to
/// wait for the answer, and `None` — the user closing the dialog — is an
/// ordinary outcome rather than an error.
async fn chosen_path(
    rx: tokio::sync::oneshot::Receiver<Option<FilePath>>,
) -> Result<Option<PathBuf>, String> {
    let Some(file) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    file.into_path().map(Some).map_err(|e| e.to_string())
}

/// Upper bound on a backup file, enforced by `import_backup` below.
const MAX_BACKUP_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Serialize)]
pub struct BackupExport {
    pub path: String,
    pub totals: backup::Totals,
}

/// Writes every character — persona, long-term memories and stored
/// conversations — along with the settings this install connects with, to a
/// file the user picks, for carrying to another device.
///
/// `include_api_key` decides whether the key itself goes in. The frontend
/// asks before calling, because the answer is the difference between an
/// ordinary document and a credential the user is about to drop in a folder;
/// `false` still carries the workspace, region, hotkey and preferences, which
/// is what you want when the file is going to someone else.
///
/// `Ok(None)` means the save dialog was dismissed, so there is nothing to
/// report.
#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    include_api_key: bool,
) -> Result<Option<BackupExport>, String> {
    let api_key = include_api_key.then(secrets::get_api_key).flatten();
    // Snapshot first, dialog second. The read takes milliseconds; the dialog
    // is up for as long as the user browses for a folder, and the live
    // session needs the database in the meantime — so the lock must not be
    // held across that await.
    let payload = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        backup::export(&conn, api_key).map_err(|e| e.to_string())?
    };
    let json = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    backup_dialog(&app)
        .set_title(crate::tr!("Back up characters", "备份角色"))
        .set_file_name(format!(
            "voicechat-backup-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M")
        ))
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = chosen_path(rx).await? else {
        return Ok(None);
    };
    // The platform dialogs append the filter's extension to a name typed
    // without one, but not every one of them does — make it deterministic so
    // the file is always something the import filter will show again.
    let path = if path.extension().is_some() {
        path
    } else {
        path.with_extension("json")
    };

    std::fs::write(&path, json).map_err(|e| {
        crate::tr!(
            format!("Couldn't write {}: {e}", path.display()),
            format!("无法写入 {}：{e}", path.display()),
        )
    })?;
    Ok(Some(BackupExport {
        path: path.display().to_string(),
        totals: payload.totals(),
    }))
}

/// Restores a backup file into this device, merging by id: characters the
/// file brings that aren't here yet are added, ones that are here take the
/// file's version along with its memories and conversations. Nothing already
/// on this device is deleted.
///
/// Settings in the file replace this device's — the API key included, when it
/// carries one — because a restore that left you retyping the key and hunting
/// for your workspace id would not be a restore.
///
/// `Ok(None)` means the picker was dismissed.
#[tauri::command]
pub async fn import_backup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<backup::ImportSummary>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    backup_dialog(&app)
        .set_title(crate::tr!("Restore from backup", "从备份恢复"))
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = chosen_path(rx).await? else {
        return Ok(None);
    };

    // Checked before reading rather than after, because the whole file is
    // held in memory twice over — once as text, once parsed — and the point
    // of a cap is not to get that far. A real backup of a heavily used
    // account is a few megabytes; this leaves two orders of magnitude of
    // room and still refuses a file picked to exhaust memory.
    let size = std::fs::metadata(&path)
        .map_err(|e| {
            crate::tr!(
                format!("Couldn't read {}: {e}", path.display()),
                format!("无法读取 {}：{e}", path.display()),
            )
        })?
        .len();
    if size > MAX_BACKUP_BYTES {
        return Err(crate::tr!(
            format!(
                "That file is {} MB — too large to be a VoiceChat backup (the limit is {} MB)",
                size / 1_048_576,
                MAX_BACKUP_BYTES / 1_048_576
            ),
            format!(
                "该文件有 {} MB，超出了 VoiceChat 备份的大小上限（{} MB）",
                size / 1_048_576,
                MAX_BACKUP_BYTES / 1_048_576
            ),
        ));
    }

    let raw = std::fs::read_to_string(&path).map_err(|e| {
        crate::tr!(
            format!("Couldn't read {}: {e}", path.display()),
            format!("无法读取 {}：{e}", path.display()),
        )
    })?;
    let parsed: backup::Backup = serde_json::from_str(&raw).map_err(|e| {
        crate::tr!(
            format!("This file isn't a readable VoiceChat backup: {e}"),
            format!("无法读取该 VoiceChat 备份文件：{e}"),
        )
    })?;
    parsed.check_compatible()?;
    parsed.validate()?;

    let mut summary = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        backup::import(&mut conn, &parsed).map_err(|e| e.to_string())?
    };

    // The settings that are more than a database row. `backup::import` stored
    // the rest inside its transaction; these three reach into the OS keyring,
    // this process's display language and the OS's global shortcut table, so
    // they are applied out here where each can fail on its own terms without
    // rolling back characters that have already landed.
    let mut key_error = None;
    if let Some(settings) = &parsed.settings {
        if let Some(tag) = settings.ui_language.as_deref() {
            i18n::set(Lang::from_tag(tag));
        }
        if let Some(hotkey) = settings.hotkey.as_deref() {
            let wanted = Some(hotkey.trim().to_string()).filter(|s| !s.is_empty());
            // The combo may already belong to another app on this machine.
            // `apply_hotkey` leaves the current one working when that
            // happens, and a hotkey is not worth failing a restore over — the
            // user can pick another one in Settings.
            if let Err(e) = apply_hotkey(&app, &state, wanted) {
                tracing::warn!("couldn't register the restored hotkey {hotkey:?}: {e}");
            }
        }
        if let Some(key) = settings.api_key.as_deref() {
            match secrets::set_api_key(key.trim()) {
                Ok(()) => summary.api_key_restored = true,
                // Held rather than returned, so the reconnect below still
                // happens: everything else about this restore worked.
                Err(e) => key_error = Some(e),
            }
        }
    }

    // The character that is current may have just been given a new persona,
    // voice and set of memories — none of which an open connection would
    // pick up on its own — and on a fresh install there was no current
    // character at all until this import. Reconnecting covers both, and the
    // `chat:character` it emits is also what makes an open Chat tab refetch
    // its history list, which the restored conversations just landed in.
    let current = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let stored = db::get_setting(&conn, "current_character_id")
            .map_err(|e| e.to_string())?
            .filter(|s| !s.is_empty());
        match stored {
            Some(id) => Some(id),
            None => {
                let first = character::list(&conn)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .next()
                    .map(|c| c.id);
                if let Some(id) = &first {
                    db::set_setting(&conn, "current_character_id", id)
                        .map_err(|e| e.to_string())?;
                }
                first
            }
        }
    };
    if let Some(id) = current {
        state.session.switch_character(id);
    }

    // Reported as a failure even though the characters are in, because the
    // one thing the user has to do about it — type the key in by hand — only
    // happens if they hear about it.
    if let Some(e) = key_error {
        return Err(crate::tr!(
            format!(
                "Your characters and settings were restored, but the API key in the file couldn't be saved to the credential store, so enter it above by hand: {e}"
            ),
            format!("角色与设置已恢复，但备份中的 API key 无法写入系统凭据管理器，请在上方手动填写：{e}"),
        ));
    }
    Ok(Some(summary))
}
