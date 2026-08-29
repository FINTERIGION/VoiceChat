import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Character,
  CharacterInput,
  ChatStateEvent,
  ConnectionSettings,
  DesignPreviewResult,
  ManagedVoice,
  Memory,
  PersonaPolish,
  RegionOption,
  SecretStatus,
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
  interrupt: () => invoke<void>("interrupt"),
  setRecording: (on: boolean) => invoke<void>("set_recording", { on }),

  listMemories: (characterId: string) =>
    invoke<Memory[]>("list_memories", { characterId }),
  updateMemory: (id: string, content: string) =>
    invoke<Memory>("update_memory", { id, content }),
  deleteMemory: (id: string) => invoke<void>("delete_memory", { id }),
  clearMemories: (characterId: string) =>
    invoke<void>("clear_memories", { characterId }),

  listCharacters: () => invoke<Character[]>("list_characters"),
  createCharacter: (input: CharacterInput) =>
    invoke<Character>("create_character", { input }),
  updateCharacter: (id: string, input: CharacterInput) =>
    invoke<Character>("update_character", { id, input }),
  deleteCharacter: (id: string) => invoke<void>("delete_character", { id }),
  getCurrentCharacterId: () =>
    invoke<string | null>("get_current_character_id"),
  switchCharacter: (id: string) => invoke<void>("switch_character", { id }),
  polishPersona: (description: string) =>
    invoke<PersonaPolish>("polish_persona", { description }),

  listPresetVoices: () => invoke<string[]>("list_preset_voices"),
  startRecording: () => invoke<void>("start_recording"),
  stopRecording: () => invoke<string>("stop_recording"),
  cloneVoice: (prefix: string, audioUrl: string) =>
    invoke<string>("clone_voice", { prefix, audioUrl }),
  designVoicePreview: (voicePrompt: string, previewText: string, prefix: string) =>
    invoke<DesignPreviewResult>("design_voice_preview", {
      voicePrompt,
      previewText,
      prefix,
    }),
  slugify: (input: string) => invoke<string>("slugify", { input }),

  listVoices: () => invoke<ManagedVoice[]>("list_voices"),
  deleteVoice: (voiceId: string) => invoke<void>("delete_voice", { voiceId }),
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
 * Fired when the live session switches to a different character (never on
 * the initial connect or on an error/rolling reconnect of the same one).
 */
export function onChatCharacter(
  handler: (characterId: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("chat:character", (e) => handler(e.payload));
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
