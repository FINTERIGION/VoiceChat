use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri_plugin_updater::Update;

use crate::realtime::session::SessionHandle;
use crate::voice::clone::RecorderHandle;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub session: SessionHandle,
    pub recorder: Mutex<Option<RecorderHandle>>,
    /// Where character pictures live; see `avatar`.
    pub avatars_dir: PathBuf,
    /// Where the audio each custom voice was cloned from lives; see
    /// `voice::sample`.
    pub voice_samples_dir: PathBuf,
    /// The release the last `check_for_update` found, for `install_update`
    /// to download — so what gets installed is the version the user was shown.
    pub pending_update: Mutex<Option<Update>>,
}
