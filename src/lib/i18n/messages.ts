import { en } from "./en";
import { zhCN } from "./zh";

/** Display language of the app's own interface, as a BCP-47 tag. */
export type UiLanguage = "en" | "zh-CN";

/** Matches `i18n::DEFAULT` on the Rust side. */
export const DEFAULT_UI_LANGUAGE: UiLanguage = "en";

/**
 * Option labels are deliberately the same in every locale: someone who has
 * the app in a language they can't read still needs to recognise the entry
 * for the one they want.
 */
export const UI_LANGUAGES: { id: UiLanguage; label: string }[] = [
  { id: "en", label: "English" },
  { id: "zh-CN", label: "简体中文" },
];

export type Messages = Record<keyof typeof en, string>;
export type MessageKey = keyof typeof en;

export const CATALOGUES: Record<UiLanguage, Messages> = {
  en,
  "zh-CN": zhCN,
};

export function isUiLanguage(value: string): value is UiLanguage {
  return value in CATALOGUES;
}

/**
 * Fills `{placeholder}` slots in a message. Missing vars are left as-is
 * rather than blanked, so a mistake shows up as a visible `{name}` instead
 * of a sentence with a hole in it.
 */
export function translate(
  lang: UiLanguage,
  key: MessageKey,
  vars?: Record<string, string | number>,
): string {
  const message = CATALOGUES[lang][key];
  if (!vars) return message;
  return message.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  );
}
