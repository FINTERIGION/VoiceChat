use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct SessionConfig {
    pub modalities: Vec<String>,
    pub voice: String,
    pub instructions: String,
    pub input_audio_format: String,
    pub output_audio_format: String,
    pub turn_detection: Option<TurnDetection>,
    pub max_history_turns: u32,
}

/// Server-VAD hands-free mode. Always used — there is no push-to-talk
/// alternative — with `threshold`/`silence_duration_ms` sourced from the
/// user's global settings (`vad_threshold`/`vad_silence_ms` in the
/// `settings` table), not per-character.
///
/// `create_response` is sent as `false`, but observed behavior is that this
/// server auto-creates a response the instant its own VAD detects speech has
/// stopped regardless of this flag. Because of that, the client deliberately
/// never sends its own `response.create` (see `ServerAction::CommitTurn` in
/// `session.rs`) — only `input_audio_buffer.commit` — and instead tracks
/// `is_responding`/turn counts/rolling off the first actual response content
/// event (`session::note_turn_started`), since that's the only reliable
/// signal a response really started.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TurnDetection {
    #[serde(rename = "server_vad")]
    ServerVad {
        threshold: f32,
        silence_duration_ms: u32,
        create_response: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionConfig },
    #[serde(rename = "input_audio_buffer.append")]
    InputAudioBufferAppend { audio: String },
    #[serde(rename = "input_audio_buffer.commit")]
    InputAudioBufferCommit {},
    #[serde(rename = "response.cancel")]
    ResponseCancel {},
    /// Asks the server to speak while nobody has talked yet. Allowed in
    /// server-VAD mode only when no response is already generating. The
    /// words come from `instructions` — this event cannot carry its own
    /// prompt without overriding the session one — so it is sent only when
    /// those instructions already contain the one-sentence open-loop line.
    #[serde(rename = "response.create")]
    ResponseCreate {},
    /// Payload shape is undocumented upstream; memory injection deliberately
    /// avoids this event (see prompt::builder) and goes through `instructions`
    /// instead, so nothing constructs this variant yet.
    #[allow(dead_code)]
    #[serde(rename = "conversation.item.create")]
    ConversationItemCreate { item: serde_json::Value },
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrorPayload {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "session.created")]
    SessionCreated {},
    #[serde(rename = "input_audio_buffer.speech_started")]
    SpeechStarted {},
    #[serde(rename = "input_audio_buffer.speech_stopped")]
    SpeechStopped {},
    #[serde(rename = "response.audio.delta")]
    ResponseAudioDelta { delta: String },
    #[serde(rename = "response.audio_transcript.delta")]
    ResponseAudioTranscriptDelta { delta: String },
    #[serde(rename = "response.audio_transcript.done")]
    ResponseAudioTranscriptDone {
        #[serde(default)]
        transcript: Option<String>,
    },
    #[serde(rename = "conversation.item.input_audio_transcription.completed")]
    InputAudioTranscriptionCompleted {
        #[serde(default)]
        transcript: String,
    },
    #[serde(rename = "response.done")]
    ResponseDone {},
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        error: ErrorPayload,
    },
    /// Catch-all for event types this client does not (yet) model. The realtime
    /// protocol's server-event list grows as the model iterates, so an unrecognized
    /// `type` must be logged and ignored rather than fail deserialization and drop
    /// the whole connection.
    #[serde(other)]
    Unknown,
}
