use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_updater::UpdaterExt;

use crate::app::state::AppState;
use crate::avatar::{self, generate::ImageClient};
use crate::i18n::{self, Lang};
use crate::llm::flash::FlashClient;
use crate::secrets::{self, SecretStatus};
use crate::store::share::{SharedCharacter, SharedVoice};
use crate::store::{self, backup, character, db, memory, message, share};
use crate::subtitle;
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
pub fn set_ui_language(
    app: AppHandle,
    state: State<AppState>,
    language: String,
) -> Result<(), String> {
    let lang = Lang::from_tag(&language);
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "ui_language", lang.tag()).map_err(|e| e.to_string())?;
    }
    i18n::set(lang);
    // The tray menu is the one piece of backend UI that's on screen
    // persistently, so it's relabelled now rather than on next launch.
    if let Err(e) = crate::tray::refresh_language(&app) {
        tracing::error!("failed to relabel tray menu: {e}");
    }
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
    Ok(VadSettings {
        threshold,
        silence_ms,
    })
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

// ---- Desktop subtitle ----

#[tauri::command]
pub fn get_subtitle_settings(
    state: State<AppState>,
) -> Result<subtitle::SubtitleSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    subtitle::get_settings(&conn).map_err(|e| e.to_string())
}

/// `async` isn't a style choice here: `WebviewWindowBuilder::build` (inside
/// `subtitle::open`) deadlocks on Windows when called from a synchronous
/// command — see the builder's own docs — so this has to run off whatever
/// thread commands normally execute on.
#[tauri::command]
pub async fn set_subtitle_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: subtitle::SubtitleSettings,
) -> Result<(), String> {
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        subtitle::set_settings(&conn, settings).map_err(|e| e.to_string())?;
    }
    if settings.enabled {
        subtitle::open(&app)
    } else {
        subtitle::close(&app)
    }
}

#[tauri::command]
pub fn set_subtitle_adjusting(app: AppHandle, on: bool) -> Result<(), String> {
    subtitle::set_adjusting(&app, on)
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

/// Ends the conversation the live session is writing to (named, and
/// summarized into memory if it counts toward it, same as any other end) and
/// starts a fresh one for the same character. A no-op before any connection
/// has opened one.
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

/// A picture the character form hands back is only kept if it names one
/// that is actually stored here. Anything else — a file removed from under
/// the app, a name restored by an older build that didn't carry pictures —
/// becomes "no avatar", rather than an error that would stop the user from
/// saving the rest of their edits.
fn existing_avatar(state: &AppState, avatar_path: Option<String>) -> Option<String> {
    avatar_path.filter(|name| avatar::exists(&state.avatars_dir, name))
}

/// Whether anything the live session was built from changed — everything
/// but the picture, which the model never sees.
fn affects_session(before: &character::Character, after: &character::Character) -> bool {
    before.name != after.name
        || before.language != after.language
        || before.persona != after.persona
        || before.speech_habits != after.speech_habits
        || before.voice_kind != after.voice_kind
        || before.voice_id != after.voice_id
        || before.voice_prompt != after.voice_prompt
        || before.memory_enabled != after.memory_enabled
        || before.max_history_turns != after.max_history_turns
}

/// Deletes the picture `before` had, if `after` no longer uses it.
fn release_old_avatar(state: &AppState, before: Option<&str>, after: Option<&str>) {
    if let Some(old) = before.filter(|old| Some(*old) != after) {
        avatar::remove(&state.avatars_dir, old);
    }
}

#[tauri::command]
pub fn create_character(
    state: State<AppState>,
    mut input: character::CharacterInput,
) -> Result<character::Character, String> {
    input.name = character::check_name(&input.name)?;
    input.avatar_path = existing_avatar(&state, input.avatar_path.take());
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    character::create(&conn, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_character(
    state: State<AppState>,
    id: String,
    mut input: character::CharacterInput,
) -> Result<character::Character, String> {
    input.name = character::check_name(&input.name)?;
    input.avatar_path = existing_avatar(&state, input.avatar_path.take());
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let not_found = || crate::tr!("Character not found", "角色不存在").to_string();
    let before = character::get(&conn, &id)
        .map_err(|e| e.to_string())?
        .ok_or_else(not_found)?;
    let updated = character::update(&conn, &id, input)
        .map_err(|e| e.to_string())?
        .ok_or_else(not_found)?;
    release_old_avatar(
        &state,
        before.avatar_path.as_deref(),
        updated.avatar_path.as_deref(),
    );

    // `voice`/`instructions` only take effect on a connection's first
    // `session.update`, so if we just edited the character that's currently
    // live, the running WS won't pick up the change (new language, persona,
    // voice, etc.) until something else reconnects it. Force that now —
    // unless nothing it uses changed: a reconnect ends the conversation and
    // clears the Chat tab's transcript, far too much to pay for a new
    // picture.
    let is_current = db::get_setting(&conn, "current_character_id")
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some(id.as_str());
    if is_current && affects_session(&before, &updated) {
        state.session.switch_character(id);
    }

    Ok(updated)
}

/// Changes only a character's picture, straight from the Characters tab.
/// `None` removes it.
#[tauri::command]
pub fn set_character_avatar(
    state: State<AppState>,
    id: String,
    avatar_path: Option<String>,
) -> Result<character::Character, String> {
    let avatar_path = existing_avatar(&state, avatar_path);
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let not_found = || crate::tr!("Character not found", "角色不存在").to_string();
    let before = character::get(&conn, &id)
        .map_err(|e| e.to_string())?
        .ok_or_else(not_found)?;
    let updated = character::set_avatar(&conn, &id, avatar_path.as_deref())
        .map_err(|e| e.to_string())?
        .ok_or_else(not_found)?;
    release_old_avatar(
        &state,
        before.avatar_path.as_deref(),
        updated.avatar_path.as_deref(),
    );
    Ok(updated)
}

#[tauri::command]
pub fn delete_character(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let current = db::get_setting(&conn, "current_character_id").map_err(|e| e.to_string())?;
    let picture = character::get(&conn, &id)
        .map_err(|e| e.to_string())?
        .and_then(|c| c.avatar_path);
    character::delete(&conn, &id).map_err(|e| e.to_string())?;
    if let Some(picture) = picture {
        avatar::remove(&state.avatars_dir, &picture);
    }

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

// ---- Avatars ----

/// Stores a picture the editor has cropped (a `data:` URL) and returns the
/// name to put in the character's `avatar_path`. Nothing points at it until
/// the character is saved; one that never is gets swept at the next launch.
#[tauri::command]
pub fn save_avatar(state: State<AppState>, data_url: String) -> Result<String, String> {
    let bytes = avatar::decode_data_url(&data_url)?;
    avatar::save(&state.avatars_dir, &bytes)
}

/// Draws a picture for `prompt` with Qwen-Image and returns it as a `data:`
/// URL, uncropped and unsaved — the editor puts it through the same crop as
/// a picture the user picked themselves, and saves it from there.
#[tauri::command]
pub async fn generate_avatar(
    state: State<'_, AppState>,
    prompt: String,
) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(crate::tr!(
            "Describe what the avatar should look like first",
            "请先描述头像的样子",
        )
        .into());
    }
    let api_key = secrets::get_api_key()
        .ok_or_else(|| crate::tr!("No API key configured yet", "尚未配置 API key"))?;
    let (workspace_id, region) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        (
            db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
            db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
        )
    };
    let (bytes, format) = ImageClient::new(api_key, workspace_id, region.as_deref())
        .generate(prompt)
        .await?;
    Ok(avatar::to_data_url(&bytes, format))
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

/// Keeps `sample_url` — the `data:` URL a voice was just cloned from — as
/// that voice's sample, so a character using it can later be shared without
/// the user supplying the audio again (see `voice::sample`).
///
/// Never fails the clone: the voice already exists in the account by now,
/// and a sample that can't be kept (a public URL, a format or size sharing
/// wouldn't take, a disk error) only means sharing will ask for one.
fn keep_sample(state: &AppState, voice_id: &str, sample_url: &str) {
    let Some(bytes) = voice::sample::from_data_url(sample_url) else {
        tracing::info!("not keeping the sample for {voice_id}: it isn't one sharing can use");
        return;
    };
    let kept = voice::sample::save(&state.voice_samples_dir, &bytes).and_then(|name| {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        match store::voice_sample::set(&conn, voice_id, &name) {
            Ok(replaced) => Ok(replaced),
            Err(e) => {
                voice::sample::remove(&state.voice_samples_dir, &name);
                Err(e.to_string())
            }
        }
    });
    match kept {
        Ok(Some(replaced)) => voice::sample::remove(&state.voice_samples_dir, &replaced),
        Ok(None) => {}
        Err(e) => tracing::warn!("couldn't keep the sample for {voice_id}: {e}"),
    }
}

/// Clones a voice for the realtime model from `audio_url` — a recording, a
/// picked file, or a designed voice's preview — and keeps the audio as the
/// new voice's sample.
#[tauri::command]
pub async fn clone_voice(
    state: State<'_, AppState>,
    prefix: String,
    audio_url: String,
) -> Result<String, String> {
    let voice_id = voice_service(&state)?
        .clone_voice(voice::service::REALTIME_TARGET_MODEL, &prefix, &audio_url)
        .await?;
    keep_sample(&state, &voice_id, &audio_url);
    Ok(voice_id)
}

/// The audio `voice_id` was cloned from, as a `data:` URL, if it was kept —
/// voices cloned before samples were, or somewhere other than this app,
/// have none.
#[tauri::command]
pub fn get_voice_sample(
    state: State<AppState>,
    voice_id: String,
) -> Result<Option<String>, String> {
    let name = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        store::voice_sample::get(&conn, &voice_id).map_err(|e| e.to_string())?
    };
    Ok(name
        .and_then(|name| voice::sample::read(&state.voice_samples_dir, &name))
        .map(|(bytes, format)| voice::sample::to_data_url(&bytes, format)))
}

#[derive(Serialize)]
pub struct DesignPreviewResult {
    /// The TTS-series voice this preview enrolled, for the design tab to
    /// hand to `discard_design_previews` once it is done with it.
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

/// Deletes a TTS-series voice Voice Design enrolled. Nothing needs one once
/// its preview audio is in hand: the realtime voice is cloned from the
/// audio, and the live session can't speak with a TTS-series voice at all.
///
/// Failing to is only logged. What it leaves is a voice the cloud tab lists
/// as unusable, not anything a character depends on.
async fn discard_design_voice(service: &VoiceService, voice_id: &str) {
    if let Err(e) = service.delete_voice(voice_id).await {
        tracing::warn!("couldn't delete the design step's voice {voice_id}: {e}");
    }
}

/// Deletes the voices the design tab's previews enrolled
/// (`design_voice_preview`), once a voice has been made from one of them or
/// the tab has been left without one.
///
/// Refuses anything shaped like a realtime voice — the only kind a character
/// can speak with — so a wrong id from the frontend can't cost one.
#[tauri::command]
pub async fn discard_design_previews(
    state: State<'_, AppState>,
    voice_ids: Vec<String>,
) -> Result<(), String> {
    let service = voice_service(&state)?;
    for id in &voice_ids {
        if id.starts_with(voice::service::REALTIME_TARGET_MODEL) {
            tracing::warn!("not discarding {id}: it's a realtime voice, not a design preview");
            continue;
        }
        discard_design_voice(&service, id).await;
    }
    Ok(())
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
    /// TTS-series voices an account also collects: Voice Design enrolls one
    /// for every preview, and ones it couldn't delete afterwards — or made
    /// before it deleted them at all — stay behind.
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
    voice_service(&state)?.delete_voice(&voice_id).await?;

    // Only once the voice is really gone: while it exists, its sample is
    // what lets a character using it be shared.
    let sample = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        store::voice_sample::delete(&conn, &voice_id).map_err(|e| e.to_string())?
    };
    if let Some(name) = sample {
        voice::sample::remove(&state.voice_samples_dir, &name);
    }
    Ok(())
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
        .add_filter(crate::tr!("Voice Chat backup", "Voice Chat 备份"), &["json"]);
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

/// Reads a file the user picked as text, refusing one over `max_bytes`.
///
/// The size is checked before reading rather than after, because the whole
/// file is held in memory twice over — once as text, once parsed — and the
/// point of a cap is not to get that far. `too_large` gets the size in MB.
fn read_picked(
    path: &Path,
    max_bytes: u64,
    too_large: impl FnOnce(u64) -> String,
) -> Result<String, String> {
    let unreadable = |e: std::io::Error| {
        crate::tr!(
            format!("Couldn't read {}: {e}", path.display()),
            format!("无法读取 {}：{e}", path.display()),
        )
    };
    let size = std::fs::metadata(path).map_err(unreadable)?.len();
    if size > max_bytes {
        return Err(too_large(size / 1_048_576));
    }
    std::fs::read_to_string(path).map_err(unreadable)
}

/// Upper bound on a backup file, enforced by `import_backup` below.
const MAX_BACKUP_BYTES: u64 = 256 * 1024 * 1024;

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
    let mut payload = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        backup::export(&conn, api_key).map_err(|e| e.to_string())?
    };
    payload.embed_avatars(&state.avatars_dir);
    payload.embed_voice_samples(&state.voice_samples_dir);
    let json = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    backup_dialog(&app)
        .set_title(crate::tr!("Back up characters", "备份角色"))
        .set_file_name(format!(
            "voice-chat-backup-{}.json",
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

    // A real backup is a few megabytes of text, plus a few more per
    // character with a kept voice sample — tens of megabytes for someone
    // with many cloned voices. The cap leaves several times that and still
    // refuses a file picked to exhaust memory.
    let raw = read_picked(&path, MAX_BACKUP_BYTES, |mb| {
        crate::tr!(
            format!(
                "That file is {mb} MB — too large to be a Voice Chat backup (the limit is {} MB)",
                MAX_BACKUP_BYTES / 1_048_576
            ),
            format!(
                "该文件有 {mb} MB，超出了 Voice Chat 备份的大小上限（{} MB）",
                MAX_BACKUP_BYTES / 1_048_576
            ),
        )
    })?;
    let mut parsed: backup::Backup = serde_json::from_str(&raw).map_err(|e| {
        crate::tr!(
            format!("This file isn't a readable Voice Chat backup: {e}"),
            format!("无法读取该 Voice Chat 备份文件：{e}"),
        )
    })?;
    parsed.check_compatible()?;
    parsed.validate()?;
    // Written before the database transaction rather than inside it: a file
    // can't be rolled back, but one the import then fails to reference is
    // just an orphan for the next launch's sweep.
    parsed.unpack_avatars(&state.avatars_dir)?;
    parsed.unpack_voice_samples(&state.voice_samples_dir)?;

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
            format!(
                "角色与设置已恢复，但备份中的 API key 无法写入系统凭据管理器，请在上方手动填写：{e}"
            ),
        ));
    }
    Ok(Some(summary))
}

// ---- Sharing characters ----

/// Like `backup_dialog`, filtered to shared character files instead.
fn character_file_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut builder = app
        .dialog()
        .file()
        .add_filter(crate::tr!("Voice Chat character", "Voice Chat 角色"), &["json"]);
    if let Some(window) = app.get_webview_window("main") {
        builder = builder.set_parent(&window);
    }
    builder
}

/// Upper bound on a shared character file: room for the largest avatar and
/// voice sample `share` accepts, base64-encoded, with plenty to spare.
const MAX_CHARACTER_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The character's name as a file name, with what Windows won't allow in
/// one swapped out.
fn character_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim().trim_end_matches('.');
    format!("{}.json", if safe.is_empty() { "character" } else { safe })
}

/// Writes one character — persona, speech habits, language, the avatar if
/// `include_avatar`, and `voice` — to a file the user picks, for handing to
/// someone else to import. Memories, conversations and settings stay here.
///
/// `voice` comes from the frontend rather than being read off the character,
/// because the user may choose it: the dialog starts on the voice's kept
/// sample (`get_voice_sample`), and asks for a recording or a description
/// only when there is none — a voice cloned before samples were kept, say.
///
/// `Ok(None)` means the save dialog was dismissed.
#[tauri::command]
pub async fn export_character(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    include_avatar: bool,
    voice: SharedVoice,
) -> Result<Option<String>, String> {
    let c = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        character::get(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| crate::tr!("Character not found", "角色不存在").to_string())?
    };
    let avatar = include_avatar
        .then_some(c.avatar_path.as_deref())
        .flatten()
        .and_then(|name| avatar::read(&state.avatars_dir, name))
        .map(|(bytes, format)| avatar::to_data_url(&bytes, format));
    // Put through the same checks an import applies, so a file this writes
    // is always one `import_character` will take — and a sample the user
    // just attached that the recipient's copy couldn't be cloned from is
    // caught here, while they can still pick another. The voice goes first,
    // on its own: what's wrong there is the sample or description in the
    // dialog, not a file, which doesn't exist yet.
    let voice = voice.normalize().map_err(|why| {
        crate::tr!(
            format!("This voice can't be shared: {why}"),
            format!("无法分享这个音色：{why}"),
        )
    })?;
    let shared = SharedCharacter::from_character(&c, avatar, voice).normalize()?;
    let json = serde_json::to_string_pretty(&shared).map_err(|e| e.to_string())?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    character_file_dialog(&app)
        .set_title(crate::tr!("Share character", "分享角色"))
        .set_file_name(character_file_name(&c.name))
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = chosen_path(rx).await? else {
        return Ok(None);
    };
    // As in `export_backup`: not every platform dialog appends the filter's
    // extension to a name typed without one.
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
    Ok(Some(path.display().to_string()))
}

/// Opens a shared character file and returns what is in it, checked, for the
/// import dialog to show. Nothing is created yet: making the voice costs a
/// request against the user's account and can take a while, so the user
/// sees who they are importing before `import_character` does it.
///
/// `Ok(None)` means the picker was dismissed.
#[tauri::command]
pub async fn open_character_file(app: AppHandle) -> Result<Option<SharedCharacter>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    character_file_dialog(&app)
        .set_title(crate::tr!("Import a character", "导入角色"))
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = chosen_path(rx).await? else {
        return Ok(None);
    };

    let raw = read_picked(&path, MAX_CHARACTER_FILE_BYTES, |mb| {
        crate::tr!(
            format!(
                "That file is {mb} MB — too large to be a Voice Chat character (the limit is {} MB)",
                MAX_CHARACTER_FILE_BYTES / 1_048_576
            ),
            format!(
                "该文件有 {mb} MB，超出了 Voice Chat 角色文件的大小上限（{} MB）",
                MAX_CHARACTER_FILE_BYTES / 1_048_576
            ),
        )
    })?;
    SharedCharacter::parse(&raw)?.normalize().map(Some)
}

/// Creates a new character from a shared file's contents, as
/// `open_character_file` returned them, making its voice under this user's
/// own DashScope account on the way:
///
/// - a preset voice is used as it is;
/// - a description is put through Voice Design, and the sample that comes
///   back is cloned for the realtime model — the same two steps as the Voice
///   Studio's design tab;
/// - an audio sample is cloned directly, as in the clone tab.
///
/// The character is added alongside the others; the one being talked to
/// stays current, so importing never interrupts a conversation.
#[tauri::command]
pub async fn import_character(
    state: State<'_, AppState>,
    character: SharedCharacter,
) -> Result<character::Character, String> {
    // Checked again rather than trusted for having been checked on the way
    // out to the dialog: this is the call that acts on it.
    let shared = character.normalize()?;

    // Local first, then remote. A picture saved for an import that then
    // fails is an orphan the next launch's sweep removes; a voice created in
    // the user's account for one is not, so it is made only once everything
    // on this side has worked.
    let avatar_path = shared
        .avatar_bytes()
        .map(|bytes| avatar::save(&state.avatars_dir, &bytes))
        .transpose()?;

    let prefix = voice::slugify(&shared.name);
    let (voice_kind, voice_id, voice_prompt) = match &shared.voice {
        SharedVoice::Preset { id } => ("preset", id.clone(), None),
        SharedVoice::Description {
            prompt,
            preview_text,
        } => {
            let service = voice_service(&state)?;
            let preview_text = preview_text
                .as_deref()
                .unwrap_or_else(|| share::default_preview_text(&shared.language, prompt));
            let preview = service.design_voice(prompt, preview_text, &prefix).await?;
            let sample = format!("data:audio/wav;base64,{}", preview.preview_audio_b64);
            let cloned = service
                .clone_voice(voice::service::REALTIME_TARGET_MODEL, &prefix, &sample)
                .await;
            // Cloned from or not, the design step's voice was only ever
            // needed to read the sample out.
            if let Some(tts_voice) = &preview.tts_voice {
                discard_design_voice(&service, tts_voice).await;
            }
            let id = cloned?;
            keep_sample(&state, &id, &sample);
            ("designed", id, Some(prompt.clone()))
        }
        SharedVoice::Audio { data } => {
            let id = voice_service(&state)?
                .clone_voice(voice::service::REALTIME_TARGET_MODEL, &prefix, data)
                .await?;
            // So the imported character can be passed on in turn.
            keep_sample(&state, &id, data);
            ("cloned", id, None)
        }
    };

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    character::create(
        &conn,
        character::CharacterInput {
            name: shared.name,
            avatar_path,
            language: shared.language,
            persona: shared.persona,
            speech_habits: shared.speech_habits,
            voice_kind: voice_kind.into(),
            voice_id: Some(voice_id),
            voice_prompt,
            // The defaults a character made in the editor starts with.
            memory_enabled: shared.memory_enabled.unwrap_or(true),
            max_history_turns: shared.max_history_turns.unwrap_or(20),
        },
    )
    .map_err(|e| e.to_string())
}

// ---- Updates ----

/// A newer release than the one running, as `check_for_update` found it.
#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// The release notes, which the release workflow takes from the tag.
    pub notes: Option<String>,
    /// When it was published, RFC 3339.
    pub date: Option<String>,
}

/// How `install_update` is getting on, streamed to the page as it goes.
#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum UpdateDownloadEvent {
    Progress {
        downloaded: u64,
        total: Option<u64>,
    },
    /// Downloaded and its signature checked; the installer takes over next.
    Installing,
}

/// Reads the `latest.json` the release workflow attaches to each GitHub
/// release and resolves to what it offers — or `None` when this is already
/// the newest version.
#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<UpdateInfo>, String> {
    let exiting = app.clone();
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        // On Windows, installing starts the installer and then ends the
        // process on the spot, so `RunEvent::Exit` never fires. This does
        // what that handler would, plus the cleanup the plugin's own hook
        // does, which this one replaces.
        .on_before_exit(move || {
            exiting.state::<AppState>().session.shutdown();
            exiting.cleanup_before_exit();
        })
        .build()
        .map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| match e {
        // What the update URL answers until a release has been published.
        tauri_plugin_updater::Error::ReleaseNotFound => crate::tr!(
            "No published release was found to update from.",
            "没有找到可用于更新的已发布版本。"
        )
        .to_string(),
        e => crate::tr!(
            format!("Couldn't check for updates: {e}"),
            format!("检查更新失败：{e}")
        ),
    })?;
    let info = found.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
        date: update
            .raw_json
            .get("pub_date")
            .and_then(|date| date.as_str())
            .map(str::to_owned),
    });
    *state.pending_update.lock().map_err(|e| e.to_string())? = found;
    Ok(info)
}

/// Downloads the release the last check found, verifies its signature
/// against the public key in `tauri.conf.json`, and starts its installer,
/// which closes the app and opens it again when done. On Windows this never
/// returns `Ok`: the process ends as soon as the installer is running.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<(), String> {
    let update = state
        .pending_update
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| crate::tr!("Check for updates first.", "请先检查更新。").to_string())?;
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                // Chunks are a few KB each; a message per percent (or per
                // MiB, when the size isn't known) is plenty for a progress bar.
                let step = total.map_or(1 << 20, |total| (total / 100).max(1));
                if downloaded - reported >= step || Some(downloaded) == total {
                    reported = downloaded;
                    let _ = on_event.send(UpdateDownloadEvent::Progress { downloaded, total });
                }
            },
            || {
                let _ = on_event.send(UpdateDownloadEvent::Installing);
            },
        )
        .await
        .map_err(|e| {
            crate::tr!(
                format!("Couldn't download the update: {e}"),
                format!("下载更新失败：{e}")
            )
        })?;
    update.install(bytes).map_err(|e| {
        crate::tr!(
            format!("Couldn't start the installer: {e}"),
            format!("无法启动安装程序：{e}")
        )
    })?;
    // Only reached where installing doesn't end the process by itself.
    app.restart()
}
