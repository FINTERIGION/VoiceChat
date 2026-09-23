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

/** One row of the Chat tab's history list. */
export interface ConversationSummary {
  id: string;
  character_id: string;
  /**
   * `null` until the conversation has been named — either automatically,
   * once there is a turn to name it after, or by hand. `preview` stands in
   * for it meanwhile.
   */
  title: string | null;
  started_at: string;
  ended_at: string | null;
  message_count: number;
  /** The conversation's opening line, as a fallback label. */
  preview: string;
  /**
   * Whether it has been summarized into the character's long-term memory.
   * Every conversation is kept in the history — with memory off, or opted
   * out of for that one conversation, it just isn't remembered — so this is
   * what tells the delete dialog whether anything of it outlives deletion.
   */
  memorized: boolean;
}

/** A stored message, as replayed when reviewing a past conversation. */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  created_at: string;
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
  /** `OK`, `DEPLOYING` or `UNDEPLOYED`; only `OK` voices can be spoken with. */
  status: string | null;
  created_at: string | null;
  bound_character_id: string | null;
  bound_character_name: string | null;
  /**
   * Whether the live session can use this voice. An account also collects
   * TTS-series voices — every run of the design flow enrols one before
   * cloning its preview — and those can't drive a realtime conversation.
   */
  realtime_compatible: boolean;
}

export type MemoryKind = "profile" | "fact" | "summary" | "open_loop";

export interface Memory {
  id: string;
  character_id: string;
  kind: MemoryKind;
  content: string;
  salience: number;
  updated_at: string;
}

/** How much a backup file holds, for the line Settings shows afterwards. */
export interface BackupTotals {
  characters: number;
  memories: number;
  conversations: number;
  messages: number;
}

export interface BackupExport {
  /** Where the file was written, as the user picked it. */
  path: string;
  totals: BackupTotals;
}

export interface SubtitleSettings {
  enabled: boolean;
  translate: boolean;
}

/** A line of the live subtitle, as it streams in — see `chat:transcript`'s
 * `TranscriptEvent` for the equivalent on the main window's transcript. */
export interface SubtitleLineEvent {
  id: number;
  text: string;
  done: boolean;
}

/** One segment (roughly a sentence) of a line's translation. Arrives
 * separately from, and generally after, the part of the line it translates —
 * matched up by `id` and placed by `index`, not by arrival order. */
export interface SubtitleTranslationEvent {
  id: number;
  index: number;
  text: string;
}

export interface BackupImportSummary {
  totals: BackupTotals;
  /** Of the characters in the file, how many this device didn't already have. */
  new_characters: number;
  /**
   * Whether the file carried settings, and whether the API key was one of
   * them and is now in this device's credential store.
   */
  settings_restored: boolean;
  api_key_restored: boolean;
}
