//! Process-wide UI language for the user-facing strings the backend
//! produces — command errors, session error states, region labels.
//!
//! Deliberately *not* a message catalogue keyed by id: the backend's share
//! of the UI text is a few dozen error strings scattered across modules that
//! have no access to `AppState` (audio capture, the realtime session task),
//! so a global atomic plus the `tr!` macro keeps both translations at the
//! call site where they can be read together, with no plumbing.
//!
//! LLM prompts (`prompt::builder`, `memory`, `polish_persona`) are *not*
//! covered by this: those steer what the model says, which is the
//! character's `language` setting, not the operator's display language.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Zh,
}

/// Matches the frontend's `DEFAULT_UI_LANGUAGE`.
pub const DEFAULT: Lang = Lang::En;

const EN: u8 = 0;
const ZH: u8 = 1;

static CURRENT: AtomicU8 = AtomicU8::new(EN);

impl Lang {
    /// Unknown/malformed tags fall back to the default rather than erroring:
    /// a settings row written by a future version must never be able to stop
    /// the app from starting.
    pub fn from_tag(tag: &str) -> Self {
        match tag {
            "zh-CN" => Lang::Zh,
            _ => DEFAULT,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh-CN",
        }
    }
}

pub fn set(lang: Lang) {
    CURRENT.store(
        match lang {
            Lang::En => EN,
            Lang::Zh => ZH,
        },
        Ordering::Relaxed,
    );
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        ZH => Lang::Zh,
        _ => Lang::En,
    }
}

/// Picks the English or Simplified Chinese variant of a user-facing string
/// according to the current display language. Both arms are evaluated
/// lazily, so `crate::tr!(format!(...), format!(...))` only formats the one
/// actually used.
#[macro_export]
macro_rules! tr {
    ($en:expr, $zh:expr $(,)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Lang::En => $en,
            $crate::i18n::Lang::Zh => $zh,
        }
    };
}
