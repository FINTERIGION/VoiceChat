use keyring::Entry;
use serde::Serialize;

const SERVICE: &str = "VoiceChat";
const USERNAME: &str = "dashscope";

#[derive(Serialize)]
pub struct SecretStatus {
    pub configured: bool,
    pub tail: Option<String>,
}

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, USERNAME).map_err(|e| e.to_string())
}

pub fn status() -> SecretStatus {
    match entry().and_then(|e| e.get_password().map_err(|e| e.to_string())) {
        Ok(key) if !key.is_empty() => {
            let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            SecretStatus {
                configured: true,
                tail: Some(format!("...{tail}")),
            }
        }
        _ => SecretStatus {
            configured: false,
            tail: None,
        },
    }
}

pub fn get_api_key() -> Option<String> {
    entry().ok()?.get_password().ok()
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    entry()?.set_password(key).map_err(|e| e.to_string())
}

pub fn clear_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
