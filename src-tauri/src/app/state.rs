use std::sync::Mutex;

use rusqlite::Connection;

use crate::realtime::session::SessionHandle;
use crate::voice::clone::RecorderHandle;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub session: SessionHandle,
    pub recorder: Mutex<Option<RecorderHandle>>,
}
