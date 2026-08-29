use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::app::state::AppState;
use crate::llm::flash::FlashClient;
use crate::secrets::{self, SecretStatus};
use crate::store::{character, db, memory};
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
        return Err("API key 不能为空".into());
    }
    secrets::set_api_key(key)
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    secrets::clear_api_key()
}

#[tauri::command]
pub fn list_regions() -> Vec<crate::dashscope::RegionOption> {
    crate::dashscope::REGIONS.to_vec()
}

#[tauri::command]
pub fn get_connection_settings(state: State<AppState>) -> Result<ConnectionSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(ConnectionSettings {
        workspace_id: db::get_setting(&conn, "workspace_id").map_err(|e| e.to_string())?,
        region: db::get_setting(&conn, "region").map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub fn set_connection_settings(
    state: State<AppState>,
    settings: ConnectionSettings,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::set_setting(
        &conn,
        "workspace_id",
        settings.workspace_id.as_deref().unwrap_or(""),
    )
    .map_err(|e| e.to_string())?;
    db::set_setting(&conn, "region", settings.region.as_deref().unwrap_or(""))
        .map_err(|e| e.to_string())?;
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
    let api_key = secrets::get_api_key().ok_or("尚未配置 API key")?;
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
#[tauri::command]
pub fn set_hotkey(
    app: AppHandle,
    state: State<AppState>,
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
        crate::install_hotkey(&app, accel)?;
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
        .ok_or_else(|| "记忆不存在".to_string())
}

#[tauri::command]
pub fn delete_memory(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::delete(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_memories(state: State<AppState>, character_id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    memory::delete_all_for_character(&conn, &character_id).map_err(|e| e.to_string())
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
        .ok_or_else(|| "角色不存在".to_string())?;

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
    let api_key = secrets::get_api_key().ok_or("尚未配置 API key")?;
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
    let api_key = secrets::get_api_key().ok_or("尚未配置 API Key")?;
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
        return Err("已经在录音".into());
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
        .ok_or("尚未开始录音")?;
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
        return Err(format!("音色仍绑定在角色「{name}」上，请先在该角色里更换音色再删除"));
    }
    voice_service(&state)?.delete_voice(&voice_id).await
}
