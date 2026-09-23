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
  "common.close": "Close",

  // ---- Chat ----
  "chat.state.idle": "Idle",
  "chat.state.connecting": "Connecting",
  "chat.state.listening": "Listening",
  "chat.state.thinking": "Thinking",
  "chat.state.speaking": "Speaking",
  "chat.state.error": "Something went wrong",
  "chat.memory.on": "Memory: on",
  "chat.memory.off": "Not remembering this one",
  "chat.memory.onTitle": "This conversation will be saved to long-term memory",
  "chat.memory.offTitle":
    "This conversation won't be saved to long-term memory, but it stays in your history",
  "chat.empty":
    "Click the button below to open the mic. Your turn is sent automatically once you pause, and you can cut the AI off just by speaking…",
  "chat.emptyWithHotkey":
    "Click the button below, or press {hotkey}, to open the mic. Your turn is sent automatically once you pause, and you can cut the AI off just by speaking…",
  "chat.micOff": "Click to start talking",
  "chat.micOn": "Click to close the mic",
  "chat.transcribing": "Transcribing…",
  "chat.noCharacter": "No character selected",
  "chat.needApiKey": "Set up an API key first",
  "chat.switchCharacter": "Switch character",
  "chat.manageCharacters": "Manage characters…",
  "chat.switchedNotice": "Your conversation with {name} has ended and is saved in their history",

  // ---- First run (no API key yet) ----
  "onboarding.title": "Welcome to VoiceChat",
  "onboarding.body":
    "Before you can talk, the app needs an Alibaba Cloud Model Studio (DashScope) API key — speech recognition, the conversation and the voice all run through it.",
  "onboarding.step1": "Create an API key in the Model Studio console",
  "onboarding.step2": "Paste it under Settings › Connection and save it",
  "onboarding.step3": "Come back here and click the button below to start talking",
  "onboarding.cta": "Set up the API key",

  // ---- Chat history sidebar ----
  "history.title": "History",
  "history.collapse": "Collapse history",
  "history.expand": "Expand history",
  "history.new": "New conversation",
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
  "history.deleteBodyUnmemorized":
    "Its transcript is deleted with it and cannot be recovered.",
  "history.loadFailed": "Couldn’t open that conversation",

  // ---- Character list ----
  "characters.title": "Characters",
  "characters.new": "New character",
  "characters.current": "Current",
  "characters.noPersona": "(No persona yet)",
  "characters.noVoice": "No voice set",
  "characters.currentTitle": "The character you're talking to",
  "characters.noSpeechHabits": "(None yet)",
  "characters.startChat": "Start chatting",
  "characters.startChatTitle":
    "Switch to this character and open the chat — a conversation in progress is ended and saved to the history",
  "characters.backToChat": "Back to the chat",
  "characters.switching": "Switching…",
  "characters.stats.memory": "Long-term memory",
  "characters.stats.memoryOn": "On · {count} saved",
  "characters.stats.memoryOnPlain": "On",
  "characters.stats.memoryOff": "Off",
  "characters.stats.history": "History window",
  "characters.stats.historyValue": "{count} turns",
  "characters.stats.conversations": "Conversations",
  "characters.stats.conversationsValue": "{count}",
  "characters.stats.lastChat": "Last {when}",
  "characters.memory": "Memory",
  "characters.deleteLastTitle":
    "There has to be at least one character — create another before deleting this one",
  "characters.deleteTitle": "Delete character “{name}”?",
  "characters.deleteBody":
    "Every conversation and long-term memory belonging to this character goes with it, and cannot be recovered.",

  // ---- Avatars ----
  "avatar.label": "Avatar",
  "avatar.title": "Set avatar",
  "avatar.set": "Add an avatar",
  "avatar.change": "Change avatar",
  "avatar.remove": "Remove",
  "avatar.tab.upload": "Upload",
  "avatar.tab.generate": "Generate with AI",
  "avatar.choose": "Choose an image",
  "avatar.chooseHint": "PNG, JPEG, WebP… up to 20 MB — you can crop it next",
  "avatar.notImage": "That file isn't an image",
  "avatar.tooLarge": "That image is over {mb} MB",
  "avatar.unreadable": "Couldn't open that image — try a PNG or JPEG instead",
  "avatar.describe": "Describe the avatar",
  "avatar.describePlaceholder":
    "e.g. a girl with short silver hair and round glasses, smiling",
  "avatar.style": "Style",
  "avatar.style.anime": "Anime",
  "avatar.style.realistic": "Realistic",
  "avatar.style.cartoon3d": "3D cartoon",
  "avatar.style.watercolor": "Watercolor",
  "avatar.style.flat": "Flat illustration",
  "avatar.stylePrompt.anime":
    "Japanese anime-style illustration, clean line art, soft cel shading",
  "avatar.stylePrompt.realistic":
    "Realistic portrait photograph, soft natural light, shallow depth of field",
  "avatar.stylePrompt.cartoon3d":
    "3D cartoon render, rounded and cute, soft studio lighting",
  "avatar.stylePrompt.watercolor":
    "Watercolor illustration, soft brushstrokes, gentle colors",
  "avatar.stylePrompt.flat":
    "Flat vector illustration, simple shapes, clean bold colors",
  "avatar.promptSuffix":
    "Avatar composition: a single character, head and shoulders, face centered and looking at the viewer, simple clean background, no text anywhere in the image",
  "avatar.generate": "Generate",
  "avatar.generateHint":
    "Drawn by Qwen-Image (qwen-image-3.0) with your Model Studio API key; each image is billed to that account.",
  "avatar.generating": "Generating… {seconds}s",
  "avatar.generatingHint": "This usually takes 10–30 seconds",
  "avatar.regenerate": "Try again",
  "avatar.use": "Use this avatar",
  "avatar.cropHint": "Drag to reposition · scroll to zoom",
  "avatar.cropLabel": "Avatar crop area",
  "avatar.zoom": "Zoom",
  "avatar.zoomIn": "Zoom in",
  "avatar.zoomOut": "Zoom out",

  // ---- Character editor ----
  "characterEdit.newTitle": "New character",
  "characterEdit.editTitle": "Edit character",
  "characterEdit.name": "Name",
  "characterEdit.nameRequired": "Please give the character a name",
  "characterEdit.nameTooLong":
    "Names can be up to 12 Chinese characters or 24 Latin letters",
  "characterEdit.nameWidthTitle": "Chinese characters count as 2",
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
  "characterEdit.polished":
    "Persona and speech habits were replaced with the AI's version.",
  "characterEdit.undoPolish": "Undo",
  "characterEdit.discardTitle": "Discard unsaved changes?",
  "characterEdit.discardBody":
    "Your changes to this character haven't been saved and will be lost if you leave now.",
  "characterEdit.discardConfirm": "Discard changes",

  // The language a character speaks — unrelated to the display language.
  "language.zh": "Chinese",
  "language.ja": "Japanese",
  "language.en": "English",
  "language.auto": "Follow the user",

  "voiceKind.preset": "Preset voice",
  "voiceKind.cloned": "Cloned voice",
  "voiceKind.designed": "Designed voice",

  // ---- Voice studio ----
  "voice.title": "Voice Studio",
  "voice.tab.preset": "Presets",
  "voice.tab.cloud": "Cloud voices",
  "voice.tab.clone": "Clone a recording",
  "voice.tab.design": "Design from text",
  "voice.current": "Current",
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
  "voice.clone.start": "Start recording",
  "voice.clone.stop": "Stop recording ({elapsed} / {max})",
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
    "No memories yet. Switch on “Enable long-term memory” and chat for a few turns — summaries, open threads and facts will start showing up here.",
  "memory.kind.summary": "Summary",
  "memory.kind.fact": "Fact",
  "memory.kind.profile": "Profile",
  "memory.kind.openLoop": "Open thread",
  "memory.deleteTitle": "Delete this memory?",
  "memory.deleteSelectedTitle": "Delete the {count} selected memories?",
  "memory.deleteSelectedBody":
    "They are deleted permanently. Later conversations start building memories up again from scratch.",

  // ---- Settings ----
  "settings.title": "Settings",
  "settings.nav.general": "General",
  "settings.nav.connection": "Connection",
  "settings.nav.conversation": "Conversation",
  "settings.nav.connectionMissing": "No API key configured yet",
  "settings.autosave": "Changes are saved as you make them",
  "settings.language.heading": "Display language",
  "settings.language.hint":
    "Applies to the app's own interface. What language a character speaks is set on that character.",
  "settings.apiKey.heading": "DashScope API Key",
  "settings.apiKey.configured": "Configured ({tail})",
  "settings.apiKey.missing": "Not configured yet",
  "settings.apiKey.clear": "Clear",
  "settings.apiKey.hint":
    "Paste it in, then press Enter or click Save. You can create one in the Model Studio console.",
  "settings.apiKey.show": "Show key",
  "settings.apiKey.hide": "Hide key",
  "settings.apiKey.clearTitle": "Clear the API key?",
  "settings.apiKey.clearBody":
    "Voice chat, voice cloning and persona writing all stop working until a key is entered again.",
  "settings.optional.heading": "Endpoint (optional)",
  "settings.optional.workspaceId": "WorkspaceId",
  "settings.optional.workspacePlaceholder":
    "Leave blank to use the legacy domain",
  "settings.optional.region": "Region",
  "settings.optional.regionDefault": "Default ({label})",
  "settings.optional.regionFallback": "Beijing",
  "settings.optional.hint":
    "Voice cloning/design and realtime conversation are only available in these two regions; the one closer to you will be faster. Test connectivity again after switching.",
  "settings.vad.heading": "Hands-free detection",
  "settings.vad.hint":
    "Controls how readily the app hears you speaking while the mic is open, and how long a pause counts as you being done.",
  "settings.vad.threshold": "VAD threshold ({value})",
  "settings.vad.silence": "Silence cutoff ({ms}ms)",
  "settings.hotkey.heading": "Global hotkey",
  "settings.hotkey.hint":
    "Press it in any window to toggle the mic. Click the box below, then press the combination you want — it's saved as soon as you do.",
  "settings.hotkey.capturing": "Press the new hotkey… (Esc to cancel)",
  "settings.hotkey.disable": "Disable",
  "settings.subtitle.heading": "Desktop subtitle",
  "settings.subtitle.hint":
    "A borderless caption, always on top, showing the assistant's replies. Click-through by default, so it never blocks whatever's underneath it.",
  "settings.subtitle.enable": "Show desktop subtitle",
  "settings.subtitle.translate": "Show translation",
  "settings.subtitle.translateHint":
    "Translated into the app's display language and shown under the original line.",
  "settings.subtitle.adjust": "Adjust position…",
  "settings.subtitle.adjustDone": "Done adjusting",
  "settings.subtitle.adjustHint":
    "Drag the sample caption to where you want it, then click “Done adjusting”.",
  "subtitle.sample": "This is what your subtitle will look like",
  "settings.connectivity.heading": "Test the connection",
  "settings.connectivity.hint":
    "Makes one connection with the saved API key and endpoint.",
  "settings.connectivity.fixFirst": "Fix the endpoint settings above first.",
  "settings.connectivity.test": "Test connectivity",
  "settings.connectivity.testing": "Testing…",
  "settings.connectivity.ok": "Connected",
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
