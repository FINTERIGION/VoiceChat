use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

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
}
