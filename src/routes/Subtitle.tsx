import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useT } from "../lib/i18n";
import {
  onSubtitleAdjust,
  onSubtitleLine,
  onSubtitleSpoken,
  onSubtitleTranslation,
  subscribe,
} from "../lib/ipc";

/** How long a line stays on screen after it's finished being spoken. */
const FADE_DELAY_MS = 6000;

interface Line {
  id: number;
  text: string;
  /** Translated segments by index; may have holes while some are in flight. */
  translation: string[];
}

/** The translation as far as it's contiguous from the start — a later
 * segment that beat an earlier one back waits rather than showing out of
 * place and then being shoved along when the gap fills. */
function joinedTranslation(parts: string[]): string {
  let out = "";
  for (let i = 0; i < parts.length; i++) {
    if (parts[i] === undefined) break;
    out += parts[i];
  }
  return out;
}

export default function Subtitle() {
  const t = useT();
  const [line, setLine] = useState<Line | null>(null);
  const [visible, setVisible] = useState(false);
  const [adjusting, setAdjusting] = useState(false);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Which line is on screen right now, read by the translation handler to
  // decide whether a translation that just arrived is still current — kept
  // separate from `line` state so that check doesn't need a stale closure
  // over it.
  const currentLineId = useRef<number | null>(null);
  // Only a line that has finished playing out loud should start (or
  // restart) the fade-out countdown. Its text being complete isn't enough:
  // that arrives far ahead of the audio, and a long reply used to vanish
  // while it was still being spoken.
  const currentLineSpoken = useRef(false);

  // The window is created `transparent(true)`, but that only makes the OS
  // compositor treat it as such — the page itself still paints an opaque
  // background unless told not to.
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
  }, []);

  // (Re)starts the fade-out countdown from now. Called once when a line
  // has been spoken, and again whenever a translation lands after that, so
  // the clock always measures from whatever the user most recently had to
  // read — speech ending, or the translation's last segment catching up.
  function scheduleFade() {
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    fadeTimer.current = setTimeout(() => setVisible(false), FADE_DELAY_MS);
  }

  useEffect(() => {
    const cleanups = [
      subscribe(() =>
        onSubtitleLine((ev) => {
          // A new line replaces one that may still be counting down.
          if (currentLineId.current !== ev.id) {
            if (fadeTimer.current) {
              clearTimeout(fadeTimer.current);
              fadeTimer.current = null;
            }
            currentLineSpoken.current = false;
          }
          currentLineId.current = ev.id;
          setLine((prev) => ({
            id: ev.id,
            text: ev.text,
            // A translation only belongs to the line it was requested for —
            // a new id means a new turn, so any prior one is stale.
            translation: prev && prev.id === ev.id ? prev.translation : [],
          }));
          setVisible(ev.text.length > 0);
        }),
      ),
      subscribe(() =>
        onSubtitleSpoken((id) => {
          // Sent for every response that ends, including ones that never
          // had a line of their own, so it can repeat for a line already
          // counting down — or arrive for one already replaced.
          if (currentLineId.current !== id || currentLineSpoken.current) return;
          currentLineSpoken.current = true;
          scheduleFade();
        }),
      ),
      subscribe(() =>
        onSubtitleTranslation((ev) => {
          // A translation for a line that's already been replaced by the
          // next turn must not revive the countdown (or the box) for
          // something no longer on screen.
          if (currentLineId.current !== ev.id) return;
          setLine((prev) => {
            if (!prev || prev.id !== ev.id) return prev;
            const translation = [...prev.translation];
            translation[ev.index] = ev.text;
            return { ...prev, translation };
          });
          // The line may already have faded while the last segment was in
          // flight — bring it back so there's actually time to read the
          // pair together, not just the tail end of the original.
          if (currentLineSpoken.current) {
            setVisible(true);
            scheduleFade();
          }
        }),
      ),
      subscribe(() => onSubtitleAdjust(setAdjusting)),
    ];
    return () => {
      cleanups.forEach((cleanup) => cleanup());
      if (fadeTimer.current) clearTimeout(fadeTimer.current);
    };
  }, []);

  if (adjusting) {
    return (
      <div className="flex h-screen w-screen items-center justify-center">
        <div
          onPointerDown={() => {
            void getCurrentWindow().startDragging();
          }}
          className="cursor-move select-none rounded-2xl border-2 border-dashed border-emerald-400 bg-neutral-950/85 px-6 py-4 text-center"
        >
          <p className="text-lg font-medium text-white">{t("subtitle.sample")}</p>
          <p className="mt-2 text-xs text-emerald-400">
            {t("settings.subtitle.adjustHint")}
          </p>
        </div>
      </div>
    );
  }

  const translation = line ? joinedTranslation(line.translation).trim() : "";
  const showTranslation =
    translation.length > 0 && translation !== (line?.text.trim() ?? "");

  return (
    <div className="flex h-screen w-screen items-end justify-center pb-4">
      <div
        className={`max-w-[92%] rounded-2xl bg-neutral-950/75 px-6 py-3 text-center shadow-lg transition-opacity duration-500 ${
          visible && line ? "opacity-100" : "opacity-0"
        }`}
      >
        <p className="whitespace-pre-wrap break-words text-xl font-medium text-white">
          {line?.text}
        </p>
        {showTranslation && (
          <p className="mt-1 whitespace-pre-wrap break-words text-base text-neutral-300">
            {translation}
          </p>
        )}
      </div>
    </div>
  );
}
