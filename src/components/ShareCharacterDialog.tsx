import { useEffect, useId, useState, type ChangeEvent } from "react";
import { Share2, Undo2, Upload, X } from "lucide-react";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { describeVoice } from "../lib/labels";
import { SAMPLE_ACCEPT, SAMPLE_MAX_BYTES } from "../lib/sample";
import type { Character, SharedVoice } from "../lib/types";
import { btn, field } from "../lib/ui";
import Avatar from "./Avatar";
import Modal from "./Modal";
import RecordButton from "./RecordButton";

/** Mirrors the Voice Studio's box, and `limits::VOICE_PROMPT_CHARS`. */
const PROMPT_MAX = 500;

/**
 * What a character with no voice set speaks in — `FALLBACK_VOICE` in
 * `realtime/session.rs` — and so what it is shared as.
 */
const FALLBACK_VOICE = "longanqian";

type Source = "description" | "audio";

/**
 * Exports one character to a file someone else can import (see
 * `store::share`).
 *
 * Most of the dialog is about the voice, the one part that can't simply be
 * copied: a voice id belongs to this user's account. A preset goes as it is;
 * anything else goes as what the recipient's copy will be made from — a
 * description, or an audio sample.
 *
 * Usually that is settled before the dialog opens: every voice the app clones
 * keeps the audio it was cloned from, and the dialog starts on it, so sharing
 * is one click. Only a voice with no kept sample — cloned before the app kept
 * them, or somewhere else — needs the user: a designed one starts on its
 * description, a cloned one on recording or picking a new sample.
 */
export default function ShareCharacterDialog({
  character: c,
  onExported,
  onClose,
}: {
  character: Character;
  onExported: (path: string) => void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const presetId =
    c.voice_id === null
      ? FALLBACK_VOICE
      : c.voice_kind === "preset"
        ? c.voice_id
        : null;
  const isPreset = presetId !== null;
  const [includeAvatar, setIncludeAvatar] = useState(true);
  const [source, setSource] = useState<Source>(
    c.voice_kind === "designed" && c.voice_prompt ? "description" : "audio",
  );
  const [prompt, setPrompt] = useState(c.voice_prompt ?? "");
  const [sample, setSample] = useState<string | null>(null);
  const [sampleName, setSampleName] = useState<string | null>(null);
  // The audio the voice was cloned from, if it was kept. Held apart from
  // `sample` so a new recording can be backed out of.
  const [kept, setKept] = useState<string | null>(null);
  // True until the kept sample has been looked for, so the dialog doesn't
  // open asking for audio it is about to find.
  const [looking, setLooking] = useState(!isPreset);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isPreset || c.voice_id === null) return;
    let live = true;
    ipc
      .getVoiceSample(c.voice_id)
      .then((kept) => {
        if (!live || !kept) return;
        // The very audio the voice was cloned from beats a description even
        // for a designed voice: the recipient's copy comes out the same.
        setKept(kept);
        setSample(kept);
        setSource("audio");
      })
      .catch((e) => {
        // Nothing lost: the dialog falls back to asking for a sample.
        console.error("couldn't read the voice's kept sample", e);
      })
      .finally(() => {
        if (live) setLooking(false);
      });
    return () => {
      live = false;
    };
  }, [isPreset, c.voice_id]);

  const usingKept = kept !== null && sample === kept;
  const voice: SharedVoice | null = isPreset
    ? { kind: "preset", id: presetId }
    : looking
      ? null
      : source === "description"
      ? prompt.trim()
        ? { kind: "description", prompt: prompt.trim() }
        : null
      : sample
        ? { kind: "audio", data: sample }
        : null;

  function handleFileChange(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    // Cleared so picking the same file again after an error still fires.
    e.target.value = "";
    if (!file) return;
    setError(null);
    if (file.size > SAMPLE_MAX_BYTES) {
      setError(t("voice.clone.tooLarge", { mb: SAMPLE_MAX_BYTES / 1_048_576 }));
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      setSample(reader.result as string);
      setSampleName(file.name);
    };
    reader.onerror = () => setError(t("voice.clone.readFailed"));
    reader.readAsDataURL(file);
  }

  async function handleExport() {
    if (!voice) return;
    setBusy(true);
    setError(null);
    try {
      const path = await ipc.exportCharacter(
        c.id,
        includeAvatar && c.avatar_path !== null,
        voice,
      );
      // `null` is the save dialog dismissed: stay open, nothing happened.
      if (path) onExported(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      labelledBy={titleId}
      onClose={busy ? undefined : onClose}
      className="flex max-h-full w-full max-w-lg flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100"
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
        <h2 id={titleId} className="truncate text-sm font-medium">
          {t("share.title", { name: c.name })}
        </h2>
        <button
          onClick={onClose}
          disabled={busy}
          title={t("common.close")}
          aria-label={t("common.close")}
          className={`${btn.quiet} size-7 hover:rotate-90`}
        >
          <X className="size-4" />
        </button>
      </div>

      <div className="flex-1 space-y-5 overflow-y-auto p-5">
        <p className="text-sm leading-relaxed text-neutral-400">
          {t("share.intro")}
        </p>

        {c.avatar_path && (
          <label className="flex items-center gap-3 text-sm text-neutral-300">
            <input
              type="checkbox"
              checked={includeAvatar}
              onChange={(e) => setIncludeAvatar(e.target.checked)}
              disabled={busy}
            />
            <Avatar id={c.id} name={c.name} avatarPath={c.avatar_path} size="sm" />
            {t("share.includeAvatar")}
          </label>
        )}

        <section className="space-y-3 rounded-xl border border-neutral-800 bg-neutral-900/40 p-4">
          <p className="text-xs text-neutral-400">{t("share.voice")}</p>

          {isPreset ? (
            <p className="text-sm text-neutral-300">
              {t("share.presetHint", {
                voice: describeVoice(t, "preset", presetId),
              })}
            </p>
          ) : looking ? (
            <p className="text-sm text-neutral-500">{t("common.loading")}</p>
          ) : (
            <>
              {c.voice_kind === "cloned" && kept === null && (
                <p className="text-xs leading-relaxed text-neutral-500">
                  {t("share.clonedNote")}
                </p>
              )}
              <div role="radiogroup" className="flex gap-5 text-sm">
                {(
                  [
                    ["audio", "share.source.audio"],
                    ["description", "share.source.description"],
                  ] as const
                ).map(([value, label]) => (
                  <label key={value} className="flex items-center gap-2">
                    <input
                      type="radio"
                      name={`${titleId}-source`}
                      checked={source === value}
                      onChange={() => setSource(value)}
                      disabled={busy}
                    />
                    {t(label)}
                  </label>
                ))}
              </div>

              {source === "description" ? (
                <div className="space-y-1.5">
                  <textarea
                    value={prompt}
                    onChange={(e) => setPrompt(e.target.value.slice(0, PROMPT_MAX))}
                    rows={3}
                    disabled={busy}
                    aria-label={t("share.source.description")}
                    placeholder={t("voice.design.promptPlaceholder")}
                    className={`${field} w-full resize-y`}
                  />
                  <p className="flex justify-between gap-3 text-xs text-neutral-500">
                    <span>{t("share.descriptionHint")}</span>
                    <span className="shrink-0 tabular-nums">
                      {prompt.length}/{PROMPT_MAX}
                    </span>
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {usingKept ? (
                    <>
                      <p className="text-sm text-neutral-300">
                        {t("share.keptSample")}
                      </p>
                      {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
                      <audio controls src={kept ?? undefined} className="w-full" />
                      <p className="text-xs leading-relaxed text-neutral-500">
                        {t("share.keptHint")}
                      </p>
                      <p className="pt-2 text-xs text-neutral-500">
                        {t("share.replaceSample")}
                      </p>
                    </>
                  ) : (
                    kept !== null && (
                      <button
                        onClick={() => {
                          setError(null);
                          setSample(kept);
                          setSampleName(null);
                        }}
                        disabled={busy}
                        className={`${btn.accentGhost} gap-1 px-1.5 py-0.5 text-xs`}
                      >
                        <Undo2 className="size-3.5" />
                        {t("share.useKept")}
                      </button>
                    )
                  )}
                  <RecordButton
                    onStart={() => {
                      setError(null);
                      setSample(null);
                      setSampleName(null);
                    }}
                    onRecorded={(uri) => {
                      setSample(uri);
                      setSampleName(null);
                    }}
                    onError={setError}
                    disabled={busy}
                  />
                  {/* Visually hidden rather than `display: none`, so it can
                      still be reached with Tab; the label shows its focus. */}
                  <label
                    className={`${btn.outline} w-full gap-2 px-3 py-2 text-sm has-focus-visible:ring-2 has-focus-visible:ring-neutral-500`}
                  >
                    <Upload className="size-4 shrink-0" />
                    <span className="truncate">
                      {sampleName ?? t("share.chooseSample")}
                    </span>
                    <input
                      type="file"
                      accept={SAMPLE_ACCEPT}
                      onChange={handleFileChange}
                      disabled={busy}
                      className="sr-only"
                    />
                  </label>
                  {sample && !usingKept && (
                    // eslint-disable-next-line jsx-a11y/media-has-caption
                    <audio controls src={sample} className="w-full" />
                  )}
                  {!usingKept && (
                    <p className="text-xs leading-relaxed text-neutral-500">
                      {t("share.audioHint")}
                    </p>
                  )}
                </div>
              )}
            </>
          )}
        </section>

        {error && (
          <p role="alert" className="text-sm text-red-400">
            {error}
          </p>
        )}
      </div>

      <div className="flex justify-end gap-2 border-t border-neutral-800 px-5 py-3">
        <button
          onClick={onClose}
          disabled={busy}
          className={`${btn.outline} px-3 py-1.5 text-sm`}
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={handleExport}
          disabled={busy || voice === null}
          title={voice === null ? t("share.needVoice") : undefined}
          className={`${btn.primary} gap-1.5 px-3 py-1.5 text-sm font-medium`}
        >
          <Share2 className="size-3.5" />
          {busy ? t("share.exporting") : t("share.export")}
        </button>
      </div>
    </Modal>
  );
}
