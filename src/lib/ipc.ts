import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BackupExport,
  BackupImportSummary,
  Character,
  CharacterInput,
  ChatMessage,
  ChatStateEvent,
  ConnectionSettings,
  ConversationSummary,
  DesignPreviewResult,
  ManagedVoice,
  Memory,
  PersonaPolish,
  RegionOption,
  SecretStatus,
  SharedCharacter,
  SharedVoice,
  SubtitleLineEvent,
  SubtitleSettings,
  SubtitleTranslationEvent,
  TranscriptEvent,
  VadSettings,
} from "./types";

export const ipc = {
  getSecretStatus: () => invoke<SecretStatus>("get_secret_status"),
  setApiKey: (key: string) => invoke<void>("set_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),
  getConnectionSettings: () =>
    invoke<ConnectionSettings>("get_connection_settings"),
  setConnectionSettings: (settings: ConnectionSettings) =>
    invoke<void>("set_connection_settings", { settings }),
  listRegions: () => invoke<RegionOption[]>("list_regions"),
  getUiLanguage: () => invoke<string>("get_ui_language"),
  setUiLanguage: (language: string) =>
    invoke<void>("set_ui_language", { language }),
  getVadSettings: () => invoke<VadSettings>("get_vad_settings"),
  setVadSettings: (settings: VadSettings) =>
    invoke<void>("set_vad_settings", { settings }),
  testConnectivity: () => invoke<void>("test_connectivity"),
  startTalking: () => invoke<void>("start_talking"),
  stopTalking: () => invoke<void>("stop_talking"),
  toggleTalking: () => invoke<void>("toggle_talking"),
  getMicOpen: () => invoke<boolean>("get_mic_open"),
  getHotkey: () => invoke<string | null>("get_hotkey"),
  setHotkey: (accelerator: string | null) =>
    invoke<void>("set_hotkey", { accelerator }),
  getSubtitleSettings: () => invoke<SubtitleSettings>("get_subtitle_settings"),
  setSubtitleSettings: (settings: SubtitleSettings) =>
    invoke<void>("set_subtitle_settings", { settings }),
  setSubtitleAdjusting: (on: boolean) =>
    invoke<void>("set_subtitle_adjusting", { on }),
  interrupt: () => invoke<void>("interrupt"),
  setRecording: (on: boolean) => invoke<void>("set_recording", { on }),

  listMemories: (characterId: string) =>
    invoke<Memory[]>("list_memories", { characterId }),
  updateMemory: (id: string, content: string) =>
    invoke<Memory>("update_memory", { id, content }),
  deleteMemory: (id: string) => invoke<void>("delete_memory", { id }),
  deleteMemories: (ids: string[]) => invoke<void>("delete_memories", { ids }),

  listConversations: (characterId: string) =>
    invoke<ConversationSummary[]>("list_conversations", { characterId }),
  getConversationMessages: (id: string) =>
    invoke<ChatMessage[]>("get_conversation_messages", { id }),
  getActiveConversationId: () =>
    invoke<string | null>("get_active_conversation_id"),
  newConversation: () => invoke<void>("new_conversation"),
  renameConversation: (id: string, title: string) =>
    invoke<ConversationSummary>("rename_conversation", { id, title }),
  deleteConversation: (id: string) =>
    invoke<void>("delete_conversation", { id }),

  listCharacters: () => invoke<Character[]>("list_characters"),
  createCharacter: (input: CharacterInput) =>
    invoke<Character>("create_character", { input }),
  updateCharacter: (id: string, input: CharacterInput) =>
    invoke<Character>("update_character", { id, input }),
  deleteCharacter: (id: string) => invoke<void>("delete_character", { id }),
  getCurrentCharacterId: () =>
    invoke<string | null>("get_current_character_id"),
  switchCharacter: (id: string) => invoke<void>("switch_character", { id }),
  /** Changes the picture alone; unlike `updateCharacter`, never reconnects. */
  setCharacterAvatar: (id: string, avatarPath: string | null) =>
    invoke<Character>("set_character_avatar", { id, avatarPath }),
  /**
   * Stores a cropped picture (a `data:` URL) and resolves to the name that
   * goes in `avatar_path`. Unreferenced until a character is saved with it.
   */
  saveAvatar: (dataUrl: string) => invoke<string>("save_avatar", { dataUrl }),
  /** Draws a picture with Qwen-Image; resolves to an uncropped `data:` URL. */
  generateAvatar: (prompt: string) =>
    invoke<string>("generate_avatar", { prompt }),
  polishPersona: (description: string) =>
    invoke<PersonaPolish>("polish_persona", { description }),

  listPresetVoices: () => invoke<string[]>("list_preset_voices"),
  startRecording: () => invoke<void>("start_recording"),
  stopRecording: () => invoke<string>("stop_recording"),
  /** Also keeps the audio as the new voice's sample, for sharing later. */
  cloneVoice: (prefix: string, audioUrl: string) =>
    invoke<string>("clone_voice", { prefix, audioUrl }),
  /**
   * The audio a custom voice was cloned from, as a `data:` URL — `null` for
   * a voice cloned before samples were kept, or outside this app.
   */
  getVoiceSample: (voiceId: string) =>
    invoke<string | null>("get_voice_sample", { voiceId }),
  designVoicePreview: (voicePrompt: string, previewText: string, prefix: string) =>
    invoke<DesignPreviewResult>("design_voice_preview", {
      voicePrompt,
      previewText,
      prefix,
    }),
  /**
   * Deletes the TTS-series voices previews enrolled (`tts_voice`), once
   * nothing needs them. Failures are only logged on the Rust side.
   */
  discardDesignPreviews: (voiceIds: string[]) =>
    invoke<void>("discard_design_previews", { voiceIds }),
  slugify: (input: string) => invoke<string>("slugify", { input }),

  listVoices: () => invoke<ManagedVoice[]>("list_voices"),
  deleteVoice: (voiceId: string) => invoke<void>("delete_voice", { voiceId }),

  /**
   * Both open a native file dialog and resolve to `null` when the user
   * closes it without choosing — an ordinary outcome, not a failure.
   *
   * `includeApiKey` puts the key in the file, which is what makes the
   * restore on the other machine seamless and what makes the file worth
   * guarding — so the caller confirms it with the user first.
   */
  exportBackup: (includeApiKey: boolean) =>
    invoke<BackupExport | null>("export_backup", { includeApiKey }),
  importBackup: () => invoke<BackupImportSummary | null>("import_backup"),

  /**
   * Writes one character to a file for someone else to import, resolving
   * to where it went — or `null` if the save dialog was dismissed.
   */
  exportCharacter: (id: string, includeAvatar: boolean, voice: SharedVoice) =>
    invoke<string | null>("export_character", { id, includeAvatar, voice }),
  /**
   * Picks a shared character file and resolves to its checked contents,
   * creating nothing yet; `null` if the picker was dismissed.
   */
  openCharacterFile: () =>
    invoke<SharedCharacter | null>("open_character_file"),
  /**
   * Makes the voice under this user's account, then creates the character.
   * Can take a while when the voice has to be designed or cloned.
   */
  importCharacter: (character: SharedCharacter) =>
    invoke<Character>("import_character", { character }),
};

export function onChatState(
  handler: (event: ChatStateEvent) => void,
): Promise<UnlistenFn> {
  return listen<ChatStateEvent>("chat:state", (e) => handler(e.payload));
}

export function onChatTranscript(
  handler: (event: TranscriptEvent) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptEvent>("chat:transcript", (e) => handler(e.payload));
}

export function onChatLevel(
  handler: (level: number) => void,
): Promise<UnlistenFn> {
  return listen<number>("chat:level", (e) => handler(e.payload));
}

/**
 * `null` means the current character has long-term memory switched off, so
 * there is no per-conversation recording choice to present.
 */
export function onChatRecording(
  handler: (recording: boolean | null) => void,
): Promise<UnlistenFn> {
  return listen<boolean | null>("chat:recording", (e) => handler(e.payload));
}

export function onChatMic(
  handler: (open: boolean) => void,
): Promise<UnlistenFn> {
  return listen<boolean>("chat:mic", (e) => handler(e.payload));
}

/**
 * Fired whenever the history list may have changed — a conversation opened,
 * a message stored, a name generated, a session ended. The payload is the
 * conversation the live session is writing to right now, or `null` when it
 * isn't recording one; the list itself is refetched, since the reasons it
 * can change (a rename from another window, a deletion) are more than one
 * event could usefully describe.
 */
export function onChatConversations(
  handler: (activeId: string | null) => void,
): Promise<UnlistenFn> {
  return listen<string | null>("chat:conversations", (e) => handler(e.payload));
}

/**
 * Fired when the live session switches to a different character (never on
 * the initial connect or on an error/rolling reconnect of the same one).
 */
export function onChatCharacter(
  handler: (characterId: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("chat:character", (e) => handler(e.payload));
}

export function onSubtitleLine(
  handler: (event: SubtitleLineEvent) => void,
): Promise<UnlistenFn> {
  return listen<SubtitleLineEvent>("subtitle:line", (e) => handler(e.payload));
}

export function onSubtitleTranslation(
  handler: (event: SubtitleTranslationEvent) => void,
): Promise<UnlistenFn> {
  return listen<SubtitleTranslationEvent>("subtitle:translation", (e) =>
    handler(e.payload),
  );
}

/** The subtitle line with this id has finished playing out loud (or was cut
 * off) — as opposed to `SubtitleLineEvent.done`, which only means its text is
 * complete, usually well before the audio is. */
export function onSubtitleSpoken(
  handler: (id: number) => void,
): Promise<UnlistenFn> {
  return listen<number>("subtitle:spoken", (e) => handler(e.payload));
}

/** Whether the "调整字幕位置" mode (see Settings) is currently on. */
export function onSubtitleAdjust(
  handler: (adjusting: boolean) => void,
): Promise<UnlistenFn> {
  return listen<boolean>("subtitle:adjust", (e) => handler(e.payload));
}

/**
 * Registers a `listen()`-style subscription and returns a synchronous
 * cleanup function safe to call from a `useEffect` cleanup even before the
 * underlying async registration has resolved (React StrictMode mounts each
 * effect twice in dev, back to back, which would otherwise race: the first
 * listener's unlisten() call only fires once its registration promise
 * resolves, so without this guard it can still be live when the second
 * mount's listener is also live, delivering every event twice).
 */
export function subscribe(register: () => Promise<UnlistenFn>): () => void {
  let cancelled = false;
  let unlisten: UnlistenFn | undefined;
  register().then((f) => {
    if (cancelled) {
      f();
    } else {
      unlisten = f;
    }
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
