import type { MessageKey, Translate } from "./i18n";
import type { Language, VoiceKind } from "./types";

/** The language a character speaks — unrelated to the display language. */
export const LANGUAGE_LABEL: Record<Language, MessageKey> = {
  zh: "language.zh",
  ja: "language.ja",
  en: "language.en",
  auto: "language.auto",
};

export const VOICE_KIND_LABEL: Record<VoiceKind, MessageKey> = {
  preset: "voiceKind.preset",
  cloned: "voiceKind.cloned",
  designed: "voiceKind.designed",
};

export const PRESET_VOICE_INFO: Record<
  string,
  { name: MessageKey; desc: MessageKey }
> = {
  longanqian: {
    name: "voice.preset.longanqian.name",
    desc: "voice.preset.longanqian.desc",
  },
  longanlingxin: {
    name: "voice.preset.longanlingxin.name",
    desc: "voice.preset.longanlingxin.desc",
  },
  longanlingxi: {
    name: "voice.preset.longanlingxi.name",
    desc: "voice.preset.longanlingxi.desc",
  },
  longanxiaoxin: {
    name: "voice.preset.longanxiaoxin.name",
    desc: "voice.preset.longanxiaoxin.desc",
  },
  longanlufeng: {
    name: "voice.preset.longanlufeng.name",
    desc: "voice.preset.longanlufeng.desc",
  },
};

/**
 * A voice as a person would name it: "预置音色 · 龙安千" rather than
 * `preset · longanqian`. Custom voices have no name beyond their id.
 */
export function describeVoice(
  t: Translate,
  kind: VoiceKind,
  voiceId: string | null,
): string {
  if (voiceId === null) return t("characters.noVoice");
  const preset = kind === "preset" ? PRESET_VOICE_INFO[voiceId] : undefined;
  return `${t(VOICE_KIND_LABEL[kind])} · ${preset ? t(preset.name) : voiceId}`;
}
