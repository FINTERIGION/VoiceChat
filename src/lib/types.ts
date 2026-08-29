export interface SecretStatus {
  configured: boolean;
  tail: string | null;
}

export interface ConnectionSettings {
  workspace_id: string | null;
  region: string | null;
}

export interface RegionOption {
  id: string;
  label: string;
}

export type ChatStateKind =
  | "idle"
  | "connecting"
  | "listening"
  | "thinking"
  | "speaking"
  | "error";

export interface ChatStateEvent {
  state: ChatStateKind;
  message?: string;
}

export interface TranscriptEvent {
  role: "user" | "assistant";
  text: string;
  done: boolean;
}

export type Language = "zh" | "ja" | "en" | "auto";
export type VoiceKind = "preset" | "designed" | "cloned";

export interface Character {
  id: string;
  name: string;
  avatar_path: string | null;
  language: Language;
  persona: string;
  speech_habits: string;
  voice_kind: VoiceKind;
  voice_id: string | null;
  voice_prompt: string | null;
  memory_enabled: boolean;
  max_history_turns: number;
  created_at: string;
  updated_at: string;
}

// Hands-free voice detection is a global preference (mic/environment
// dependent), not a per-character trait.
export interface VadSettings {
  threshold: number;
  silence_ms: number;
}

export type CharacterInput = Omit<
  Character,
  "id" | "created_at" | "updated_at"
>;

export interface PersonaPolish {
  persona: string;
  speech_habits: string;
}

export interface DesignPreviewResult {
  tts_voice: string | null;
  preview_audio_data_uri: string;
}

export interface ManagedVoice {
  voice_id: string;
  status: string | null;
  created_at: string | null;
  bound_character_id: string | null;
  bound_character_name: string | null;
}

export type MemoryKind = "profile" | "fact" | "summary";

export interface Memory {
  id: string;
  character_id: string;
  kind: MemoryKind;
  content: string;
  salience: number;
  updated_at: string;
}
