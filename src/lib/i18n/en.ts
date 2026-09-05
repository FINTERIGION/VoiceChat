/**
 * The English catalogue is the source of truth: its keys define
 * `MessageKey`, so every other locale is a `Record<MessageKey, string>` and
 * a missing or misspelled translation is a type error, not a runtime hole.
 *
 * Keys are flat and dotted rather than nested objects — the lookup stays a
 * single property access, and every string for a screen reads as one block.
 * `{name}`-style placeholders are filled in by `t(key, vars)`.
 */
export const en = {
  // ---- Navigation ----
  "nav.chat": "Chat",
  "nav.characters": "Characters",
  "nav.settings": "Settings",

  // ---- Shared ----
  "common.loading": "Loading…",
  "common.save": "Save",
  "common.saving": "Saving…",
  "common.saved": "Saved",
  "common.saveFailed": "Couldn't save",
  "common.cancel": "Cancel",
  "common.delete": "Delete",
  "common.deleting": "Deleting…",
  "common.working": "Working…",
  "common.edit": "Edit",
  "common.back": "Back",
  "common.notSet": "Not set",

  // ---- Chat ----
  "chat.state.idle": "Idle",
  "chat.state.connecting": "Connecting",
  "chat.state.listening": "Listening",
  "chat.state.thinking": "Thinking",
  "chat.state.speaking": "Speaking",
  "chat.state.error": "Something went wrong",
  "chat.memory.on": "Memory: on",
  "chat.memory.off": "Not saving this one",
  "chat.memory.onTitle": "This conversation will be saved to long-term memory",
  "chat.memory.offTitle": "This conversation won't be saved to long-term memory",
  "chat.empty":
    "Click the button below to open the mic. Your turn is sent automatically once you pause, and you can cut the AI off just by speaking…",
  "chat.emptyWithHotkey":
    "Click the button below, or press {hotkey}, to open the mic. Your turn is sent automatically once you pause, and you can cut the AI off just by speaking…",
  "chat.micOff": "Click to start talking",
  "chat.micOn": "Click to close the mic",

  // ---- Chat history sidebar ----
  "history.title": "History",
  "history.new": "+ New conversation",
  "history.newDisabledTitle": "Already a new conversation — nothing to save yet",
  "history.empty": "Conversations appear here once you have talked.",
  "history.live": "Live",
  "history.untitled": "Untitled",
  "history.messageCount": "{count} messages",
  "history.backToLive": "Back to the live conversation",
  "history.viewing": "Reviewing a past conversation",
  "history.renameTitle": "Rename",
  "history.renameLabel": "Conversation name",
  "history.deleteTitle": "Delete this conversation?",
  "history.deleteBody":
    "Its transcript is deleted with it and cannot be recovered. What the character remembers from it stays in long-term memory.",
  "history.loadFailed": "Couldn’t open that conversation",

  // ---- Character list ----
  "characters.title": "Characters",
  "characters.new": "+ New character",
  "characters.current": "Current",
  "characters.noPersona": "(No persona yet)",
  "characters.noVoice": "No voice set",
  "characters.switchHint": "Double-click to switch",
  "characters.switching": "Switching…",
  "characters.memory": "Memory",
  "characters.deleteTitle": "Delete character “{name}”?",
  "characters.deleteBody":
    "Every conversation and long-term memory belonging to this character goes with it, and cannot be recovered.",

  // ---- Character editor ----
  "characterEdit.newTitle": "New character",
  "characterEdit.editTitle": "Edit character",
  "characterEdit.name": "Name",
  "characterEdit.nameRequired": "Please give the character a name",
  "characterEdit.language": "Language",
  "characterEdit.polish": "Let AI write the persona",
  "characterEdit.polishPlaceholder": "Describe this character in one sentence…",
  "characterEdit.generate": "Generate",
  "characterEdit.generating": "Generating…",
  "characterEdit.persona": "Persona",
  "characterEdit.speechHabits": "Speech habits",
  "characterEdit.voice": "Voice",
  "characterEdit.openVoiceStudio": "Open Voice Studio",
  "characterEdit.memoryEnabled": "Enable long-term memory",
  "characterEdit.maxHistoryTurns": "Max history turns ({count})",

  // The language a character speaks — unrelated to the display language.
  "language.zh": "Chinese",
  "language.ja": "Japanese",
  "language.en": "English",
  "language.auto": "Follow the user",

  // ---- Voice studio ----
  "voice.title": "Voice Studio",
  "voice.tab.preset": "Presets",
  "voice.tab.cloud": "Cloud voices",
  "voice.tab.clone": "Clone a recording",
  "voice.tab.design": "Design from text",
  "voice.preset.hint":
    "Preset voices have no online samples to audition — pick one by name.",
  "voice.preset.longanqian.name": "Long'anqian",
  "voice.preset.longanqian.desc": "Default voice",
  "voice.preset.longanlingxin.name": "Long'anlingxin",
  "voice.preset.longanlingxin.desc": "Female · warm and reassuring",
  "voice.preset.longanlingxi.name": "Long'anlingxi",
  "voice.preset.longanlingxi.desc": "Female · sweet and cute",
  "voice.preset.longanxiaoxin.name": "Long'anxiaoxin",
  "voice.preset.longanxiaoxin.desc": "Female · friendly and lively",
  "voice.preset.longanlufeng.name": "Long'anlufeng",
  "voice.preset.longanlufeng.desc": "Male · bright and cheerful",
  "voice.cloud.hint":
    "Voices already enrolled on your Alibaba Cloud account. Pick one to reuse it — nothing new is created.",
  "voice.cloud.refresh": "Refresh",
  "voice.cloud.refreshing": "Refreshing…",
  "voice.cloud.empty":
    "This account has no custom voices yet — clone or design one first.",
  "voice.cloud.bound": "In use: {name}",
  "voice.cloud.incompatible": "Text-to-speech only — can't hold a conversation",
  "voice.cloud.deploying": "Still under review",
  "voice.cloud.undeployed": "Review failed — unusable",
  "voice.cloud.unusableTitle": "This voice can't be used for live conversation",
  "voice.clone.recordHint":
    "Record 10–20 seconds of clear speech in the app (60 s max).",
  "voice.clone.start": "● Start recording",
  "voice.clone.stop": "■ Stop recording",
  "voice.clone.cloning": "Cloning…",
  "voice.clone.useRecording": "Clone from this recording",
  "voice.clone.fileHint":
    "Or pick a local audio file (10–20 seconds of clear speech).",
  "voice.clone.chooseFile": "Choose a file…",
  "voice.clone.useFile": "Clone from this file",
  "voice.clone.readFailed": "Couldn't read that file",
  "voice.design.promptLabel":
    "Voice description (Chinese or English only, 500 characters max)",
  "voice.design.promptPlaceholder":
    "e.g. a gentle, thoughtful woman's voice, slower than average, slightly nasal",
  "voice.design.previewTextLabel":
    "Sample text (150 characters or more, enough to read for 15 seconds)",
  "voice.design.previewTextPlaceholder":
    "A passage to read aloud for the sample…",
  "voice.design.generate": "Generate sample",
  "voice.design.generating": "Generating…",
  "voice.design.accept": "Sounds right — use this voice",

  // ---- Memory manager ----
  "memory.title": "{name}'s memories",
  "memory.count": "{count} in total",
  "memory.selected": "{count} selected",
  "memory.selectOne": "Select this memory",
  "memory.selectAll": "Select all",
  "memory.deselectAll": "Deselect all",
  "memory.deleteSelected": "Delete selected",
  "memory.empty":
    "No memories yet. Switch on “Enable long-term memory” and chat for a few turns — summaries and facts will start showing up here.",
  "memory.kind.summary": "Summary",
  "memory.kind.fact": "Fact",
  "memory.kind.profile": "Profile",
  "memory.deleteTitle": "Delete this memory?",
  "memory.deleteSelectedTitle": "Delete the {count} selected memories?",
  "memory.deleteSelectedBody":
    "They are deleted permanently. Later conversations start building memories up again from scratch.",

  // ---- Settings ----
  "settings.title": "Settings",
  "settings.language.heading": "Display language",
  "settings.language.hint":
    "Applies to the app's own interface. What language a character speaks is set on that character.",
  "settings.apiKey.heading": "DashScope API Key",
  "settings.apiKey.configured": "Configured ({tail})",
  "settings.apiKey.missing": "Not configured yet",
  "settings.apiKey.clear": "Clear",
  "settings.optional.heading": "Optional",
  "settings.optional.workspaceId": "WorkspaceId",
  "settings.optional.workspacePlaceholder":
    "Leave blank to use the legacy domain",
  "settings.optional.region": "Region",
  "settings.optional.regionDefault": "Default ({label})",
  "settings.optional.regionFallback": "Beijing",
  "settings.optional.hint":
    "Voice cloning/design and realtime conversation are only available in these two regions; the one closer to you will be faster. Test connectivity again after switching.",
  "settings.saveConfig": "Save settings",
  "settings.vad.heading": "Hands-free detection",
  "settings.vad.hint":
    "Controls how readily the app hears you speaking while the mic is open, and how long a pause counts as you being done.",
  "settings.vad.threshold": "VAD threshold ({value})",
  "settings.vad.silence": "Silence cutoff ({ms}ms)",
  "settings.hotkey.heading": "Global hotkey",
  "settings.hotkey.hint":
    "Press it in any window to toggle the mic. Click the button below, then press the combination you want.",
  "settings.hotkey.capturing": "Press the new hotkey…",
  "settings.hotkey.disable": "Disable",
  "settings.connectivity.test": "Test connectivity",
  "settings.connectivity.testing": "Testing…",
  "settings.connectivity.ok": "✓ Connected",
  "settings.backup.heading": "Backup & restore",
  "settings.backup.hint":
    "Writes every character — persona, long-term memories and stored conversations — plus your settings and API key into one file you can carry to another device: restore it there and the app is ready to talk, custom voices included, since those live in your DashScope account.",
  "settings.backup.export": "Back up to a file…",
  "settings.backup.exporting": "Backing up…",
  "settings.backup.import": "Restore from a file…",
  "settings.backup.importing": "Restoring…",
  "settings.backup.exported":
    "Backed up {characters} characters, {memories} memories and {messages} messages across {conversations} conversations to {path}",
  "settings.backup.imported":
    "Restored {characters} characters ({added} new here), {memories} memories and {messages} messages across {conversations} conversations.",
  "settings.backup.importedWithSettings":
    "Settings from the file were applied to this device.",
  "settings.backup.importedWithKey":
    "Settings and the API key from the file were applied to this device.",
  "settings.backup.exportTitle": "Back up to a file?",
  "settings.backup.exportBody":
    "The file will hold every character — persona, long-term memories and stored conversations — along with your workspace, region, hotkey and other settings.",
  "settings.backup.exportConfirm": "Choose where…",
  "settings.backup.includeApiKey": "Include my API key",
  "settings.backup.includeApiKeyHint":
    "Restores without retyping it. Keep the file to yourself — anyone who opens it can read the key and spend your account. Uncheck this if you are sharing these characters with someone else.",
  "settings.backup.restoreTitle": "Restore from a backup?",
  "settings.backup.restoreBody":
    "Characters this device doesn't have are added. Ones it already has are replaced by the backup's version, memories and conversations included. Nothing already here is deleted, and a conversation in progress will reconnect.\n\nSettings in the file replace this device's — the API key, workspace, region, hotkey, sensitivity and display language.\n\nOnly restore files you made yourself. A backup carries the personas and memories that tell the AI how to behave, and the key your usage is billed to, so one from someone else can change what your characters say and where your voice is sent.",
  "settings.backup.restoreConfirm": "Choose a file…",
  "settings.voices.heading": "Voice library",
  "settings.voices.refresh": "Refresh",
  "settings.voices.refreshing": "Refreshing…",
  "settings.voices.hint":
    "Custom voices made with “Clone a recording” or “Design from text” are stored in your DashScope account. A voice a character is using can't be deleted here — switch that character to another voice first.",
  "settings.voices.needApiKey": "Configure an API key first.",
  "settings.voices.empty": "No custom voices yet.",
  "settings.voices.createdUnknown": "Creation time unknown",
  "settings.voices.bound": "In use: {name}",
  "settings.voices.boundTitle":
    "Used by character “{name}”, so it can't be deleted",
  "settings.voices.deleteTitle": "Delete this voice?",
  "settings.voices.deleteBody":
    "{id} will be permanently deleted from your DashScope account and cannot be recovered; using it again would mean recording or designing it from scratch.",
} as const;
