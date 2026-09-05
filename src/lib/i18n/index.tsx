import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { ipc } from "../ipc";
import {
  DEFAULT_UI_LANGUAGE,
  isUiLanguage,
  translate,
  type MessageKey,
  type UiLanguage,
} from "./messages";

export {
  DEFAULT_UI_LANGUAGE,
  isUiLanguage,
  UI_LANGUAGES,
  type MessageKey,
  type UiLanguage,
} from "./messages";

export type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number>,
) => string;

interface I18n {
  lang: UiLanguage;
  setLang: (lang: UiLanguage) => Promise<void>;
  t: Translate;
}

const I18nContext = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  // `null` until the stored preference has been read. Children stay
  // unmounted until then so nothing renders in one language and flips to the
  // other a frame later; the read is a local SQLite lookup, so it's brief.
  const [lang, setLangState] = useState<UiLanguage | null>(null);

  useEffect(() => {
    ipc
      .getUiLanguage()
      .then((stored) =>
        setLangState(isUiLanguage(stored) ? stored : DEFAULT_UI_LANGUAGE),
      )
      .catch(() => setLangState(DEFAULT_UI_LANGUAGE));
  }, []);

  useEffect(() => {
    if (lang) document.documentElement.lang = lang;
  }, [lang]);

  // Persisting first means the backend's own user-facing strings (command
  // errors, realtime session errors, region labels) have already switched
  // over by the time the re-render fires off new requests.
  const setLang = useCallback(async (next: UiLanguage) => {
    await ipc.setUiLanguage(next);
    setLangState(next);
  }, []);

  const value = useMemo<I18n | null>(
    () =>
      lang === null
        ? null
        : {
            lang,
            setLang,
            t: (key, vars) => translate(lang, key, vars),
          },
    [lang, setLang],
  );

  if (value === null) {
    // Matches the app's background so the first paint isn't a white flash.
    return <div className="h-screen bg-neutral-950" />;
  }

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  const ctx = useContext(I18nContext);
  if (!ctx) {
    throw new Error("useI18n must be used inside <I18nProvider>");
  }
  return ctx;
}

/** Shorthand for the common case of only needing the lookup function. */
export function useT(): Translate {
  return useI18n().t;
}
