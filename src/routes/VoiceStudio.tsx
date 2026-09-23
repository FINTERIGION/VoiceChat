import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ChangeEvent,
} from "react";
import {
  Mic,
  RefreshCw,
  Sparkles,
  Square,
  Upload,
  X,
} from "lucide-react";
import Field from "../components/Field";
import Modal from "../components/Modal";
import { useI18n, useT, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { formatDateTime } from "../lib/format";
import { PRESET_VOICE_INFO } from "../lib/labels";
import type { ManagedVoice, VoiceKind } from "../lib/types";
import { btn, btnBase, cardBtn, field, hoverGlow, tabBtn } from "../lib/ui";

export interface VoiceSelection {
  voice_kind: VoiceKind;
  voice_id: string;
  voice_prompt: string | null;
}

type Tab = "preset" | "cloud" | "clone" | "design";

/** Matches `MAX_RECORD_SECS` in `voice/clone.rs`, past which nothing is kept. */
const MAX_RECORD_SECS = 60;

export default function VoiceStudio({
  characterName,
  currentVoiceId,
  onSelect,
  onClose,
}: {
  characterName: string;
  /** The voice the character has now, marked in the lists it appears in. */
  currentVoiceId: string | null;
  onSelect: (sel: VoiceSelection) => void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const [tab, setTab] = useState<Tab>("preset");

  return (
    <Modal
      labelledBy={titleId}
      onClose={onClose}
      className="flex max-h-full w-full max-w-xl flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100"
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
        <h2 id={titleId} className="text-sm font-medium">
          {t("voice.title")}
        </h2>
        <button
          onClick={onClose}
          title={t("common.close")}
          aria-label={t("common.close")}
          className={`${btn.quiet} size-7 hover:rotate-90`}
        >
          <X className="size-4" />
        </button>
      </div>

      <nav className="flex gap-1 border-b border-neutral-800 px-5 pt-2">
        {(
          [
            ["preset", "voice.tab.preset"],
            ["cloud", "voice.tab.cloud"],
            ["clone", "voice.tab.clone"],
            ["design", "voice.tab.design"],
          ] as const satisfies readonly (readonly [Tab, MessageKey])[]
        ).map(([key, label]) => (
          <button
            key={key}
            onClick={() => setTab(key)}
            className={tabBtn(tab === key)}
          >
            {t(label)}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-y-auto p-5">
        {tab === "preset" && (
          <PresetTab currentVoiceId={currentVoiceId} onSelect={onSelect} />
        )}
        {tab === "cloud" && (
          <CloudTab currentVoiceId={currentVoiceId} onSelect={onSelect} />
        )}
        {tab === "clone" && (
          <CloneTab characterName={characterName} onSelect={onSelect} />
        )}
        {tab === "design" && (
          <DesignTab characterName={characterName} onSelect={onSelect} />
        )}
      </div>
    </Modal>
  );
}

function CurrentBadge() {
  const t = useT();
  return (
    <span className="shrink-0 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-400">
      {t("voice.current")}
    </span>
  );
}

function PresetTab({
  currentVoiceId,
  onSelect,
}: {
  currentVoiceId: string | null;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const t = useT();
  const [voices, setVoices] = useState<string[]>([]);

  useEffect(() => {
    ipc.listPresetVoices().then(setVoices);
  }, []);

  return (
    <div className="space-y-2">
      <p className="text-sm text-neutral-500">{t("voice.preset.hint")}</p>
      {voices.map((v) => {
        const info = PRESET_VOICE_INFO[v];
        const isCurrent = v === currentVoiceId;
        return (
          <button
            key={v}
            onClick={() => onSelect({ voice_kind: "preset", voice_id: v, voice_prompt: null })}
            aria-current={isCurrent}
            className={`${cardBtn(isCurrent)} px-4 py-2.5 text-sm`}
          >
            <div className="flex items-baseline justify-between gap-3">
              <span className="flex items-center gap-2 font-medium">
                {info ? t(info.name) : v}
                {isCurrent && <CurrentBadge />}
              </span>
              <span className="text-xs text-neutral-500">{v}</span>
            </div>
            {info && (
              <p className="mt-0.5 text-xs text-neutral-500">{t(info.desc)}</p>
            )}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Voices that already exist on the account, listed straight from
 * DashScope's `list_voice`. Reusing one costs nothing and takes effect
 * immediately, which is the point of the tab: the clone and design tabs
 * both enrol a *new* voice every time they run, so without this a user who
 * has already made the voice they want has no way back to it.
 *
 * There is no audition here for the same reason the preset tab has none —
 * the realtime series exposes no standalone synthesis endpoint to preview
 * with, only the live session itself.
 */
function CloudTab({
  currentVoiceId,
  onSelect,
}: {
  currentVoiceId: string | null;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const t = useT();
  const [voices, setVoices] = useState<ManagedVoice[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setVoices(await ipc.listVoices());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-sm text-neutral-500">{t("voice.cloud.hint")}</p>
        <button
          onClick={refresh}
          disabled={loading}
          className={`${btn.quiet} shrink-0 gap-1.5 px-2 py-1 text-xs`}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
          {loading ? t("voice.cloud.refreshing") : t("voice.cloud.refresh")}
        </button>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}
      {voices === null && loading && (
        <p className="text-sm text-neutral-500">{t("common.loading")}</p>
      )}
      {voices !== null && voices.length === 0 && !error && (
        <p className="text-sm text-neutral-500">{t("voice.cloud.empty")}</p>
      )}

      {voices?.map((v) => (
        <CloudVoiceRow
          key={v.voice_id}
          voice={v}
          isCurrent={v.voice_id === currentVoiceId}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
}

function CloudVoiceRow({
  voice,
  isCurrent,
  onSelect,
}: {
  voice: ManagedVoice;
  isCurrent: boolean;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const { t, lang } = useI18n();
  // A missing status is treated as usable rather than blocked: it means the
  // API left the field out, not that the voice failed review.
  const blocked: MessageKey | null = !voice.realtime_compatible
    ? "voice.cloud.incompatible"
    : voice.status === "DEPLOYING"
      ? "voice.cloud.deploying"
      : voice.status === "UNDEPLOYED"
        ? "voice.cloud.undeployed"
        : null;

  return (
    <button
      onClick={() =>
        onSelect({
          // `list_voice` doesn't say which flow enrolled a voice, and the id
          // doesn't encode it either, so everything picked here is recorded
          // as cloned — true of every enrolled voice, including the ones the
          // design tab produced, whose prompt is no longer recoverable.
          voice_kind: "cloned",
          voice_id: voice.voice_id,
          voice_prompt: null,
        })
      }
      disabled={blocked !== null}
      title={blocked ? t("voice.cloud.unusableTitle") : undefined}
      aria-current={isCurrent}
      className={`${cardBtn(isCurrent)} px-4 py-2.5 text-sm`}
    >
      <p className="flex items-center gap-2">
        <span className="truncate font-mono text-xs text-neutral-300">
          {voice.voice_id}
        </span>
        {isCurrent && <CurrentBadge />}
      </p>
      <p className="mt-1 flex flex-wrap items-center gap-2 text-xs text-neutral-500">
        {voice.created_at && (
          <span>{formatDateTime(voice.created_at, lang)}</span>
        )}
        {voice.bound_character_name && (
          <span className="rounded-full bg-emerald-500/20 px-2 py-0.5 text-emerald-400">
            {t("voice.cloud.bound", { name: voice.bound_character_name })}
          </span>
        )}
        {blocked && (
          <span className="rounded-full bg-neutral-800 px-2 py-0.5 text-neutral-400">
            {t(blocked)}
          </span>
        )}
      </p>
    </button>
  );
}

function CloneTab({
  characterName,
  onSelect,
}: {
  characterName: string;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const t = useT();
  const [recording, setRecording] = useState(false);
  const [dataUri, setDataUri] = useState<string | null>(null);
  const [fileUri, setFileUri] = useState<string | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Read by the unmount cleanup, which must not re-run on every change.
  const recordingRef = useRef(false);
  // Set while `stopRecording` is in flight, so the timer hitting the cap and
  // a click on the button can't both stop the same recording.
  const stopping = useRef(false);

  useEffect(() => {
    recordingRef.current = recording;
    if (!recording) return;
    const started = Date.now();
    setElapsed(0);
    const timer = window.setInterval(
      () => setElapsed(Math.floor((Date.now() - started) / 1000)),
      250,
    );
    return () => window.clearInterval(timer);
  }, [recording]);

  // The backend keeps nothing past the cap, so carrying on would only look
  // like it is still listening.
  useEffect(() => {
    if (recording && elapsed >= MAX_RECORD_SECS) void stopRecording();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, elapsed]);

  // Closing the studio, or leaving this tab, mid-recording must not leave the
  // microphone capturing with nothing left on screen to stop it.
  useEffect(
    () => () => {
      if (recordingRef.current && !stopping.current) {
        ipc.stopRecording().catch(() => {});
      }
    },
    [],
  );

  async function stopRecording() {
    if (stopping.current) return;
    stopping.current = true;
    try {
      const uri = await ipc.stopRecording();
      setDataUri(uri);
    } catch (e) {
      setError(String(e));
    } finally {
      stopping.current = false;
      setRecording(false);
    }
  }

  async function toggleRecording() {
    setError(null);
    if (!recording) {
      try {
        await ipc.startRecording();
        setRecording(true);
        setDataUri(null);
      } catch (e) {
        setError(String(e));
      }
    } else {
      await stopRecording();
    }
  }

  async function cloneFrom(url: string) {
    setBusy(true);
    setError(null);
    try {
      const prefix = await ipc.slugify(characterName);
      const voiceId = await ipc.cloneVoice(prefix, url);
      onSelect({ voice_kind: "cloned", voice_id: voiceId, voice_prompt: null });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function handleFileChange(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setError(null);
    setFileName(file.name);
    setFileUri(null);
    const reader = new FileReader();
    reader.onload = () => setFileUri(reader.result as string);
    reader.onerror = () => setError(t("voice.clone.readFailed"));
    reader.readAsDataURL(file);
  }

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <p className="text-sm text-neutral-400">{t("voice.clone.recordHint")}</p>
        <button
          onClick={toggleRecording}
          className={`${btnBase} ${hoverGlow} w-full gap-2 rounded-lg py-2.5 text-sm font-medium ${
            recording
              ? "bg-red-500 text-neutral-950 hover:bg-red-400 hover:shadow-red-500/25"
              : "bg-neutral-800 text-neutral-100 hover:bg-neutral-700 hover:shadow-black/40"
          }`}
        >
          {recording ? (
            <Square className="size-3.5 fill-current" />
          ) : (
            <Mic className="size-4" />
          )}
          {recording
            ? t("voice.clone.stop", {
                elapsed: formatSeconds(elapsed),
                max: formatSeconds(MAX_RECORD_SECS),
              })
            : t("voice.clone.start")}
        </button>
        {dataUri && (
          <div className="space-y-2">
            {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
            <audio controls src={dataUri} className="w-full" />
            <button
              onClick={() => cloneFrom(dataUri)}
              disabled={busy}
              className={`${btn.primary} w-full py-2 text-sm font-medium`}
            >
              {busy ? t("voice.clone.cloning") : t("voice.clone.useRecording")}
            </button>
          </div>
        )}
      </div>

      <div className="space-y-2 border-t border-neutral-800 pt-4">
        <p className="text-sm text-neutral-400">{t("voice.clone.fileHint")}</p>
        {/* The input is visually hidden rather than `display: none`, so it
            can still be reached with Tab; the label shows its focus. */}
        <label
          className={`${btn.outline} w-full gap-2 px-3 py-2 text-sm has-focus-visible:ring-2 has-focus-visible:ring-neutral-500`}
        >
          <Upload className="size-4 shrink-0" />
          <span className="truncate">
            {fileName ?? t("voice.clone.chooseFile")}
          </span>
          <input
            type="file"
            accept="audio/*"
            onChange={handleFileChange}
            className="sr-only"
          />
        </label>
        {fileUri && (
          <div className="space-y-2">
            {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
            <audio controls src={fileUri} className="w-full" />
            <button
              onClick={() => cloneFrom(fileUri)}
              disabled={busy}
              className={`${btn.outline} w-full py-2 text-sm`}
            >
              {busy ? t("voice.clone.cloning") : t("voice.clone.useFile")}
            </button>
          </div>
        )}
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}
    </div>
  );
}

/** `75` → `1:15`. */
function formatSeconds(total: number): string {
  const s = Math.min(total, MAX_RECORD_SECS);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The sample length the design label recommends — enough for ~15 s. */
const PREVIEW_TEXT_MIN = 150;

function DesignTab({
  characterName,
  onSelect,
}: {
  characterName: string;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const t = useT();
  const [prompt, setPrompt] = useState("");
  const [previewText, setPreviewText] = useState("");
  const [previewUri, setPreviewUri] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function generatePreview() {
    setBusy(true);
    setError(null);
    setPreviewUri(null);
    try {
      const prefix = await ipc.slugify(characterName);
      const result = await ipc.designVoicePreview(prompt, previewText, prefix);
      setPreviewUri(result.preview_audio_data_uri);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function confirm() {
    if (!previewUri) return;
    setBusy(true);
    setError(null);
    try {
      const prefix = await ipc.slugify(characterName);
      const voiceId = await ipc.cloneVoice(prefix, previewUri);
      onSelect({ voice_kind: "designed", voice_id: voiceId, voice_prompt: prompt });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <div className="space-y-1.5">
        <Field label={t("voice.design.promptLabel")}>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value.slice(0, 500))}
            rows={3}
            placeholder={t("voice.design.promptPlaceholder")}
            className={`${field} w-full resize-y`}
          />
        </Field>
        <p className="text-right text-xs text-neutral-500">{prompt.length}/500</p>
      </div>

      <div className="space-y-1.5">
        <Field label={t("voice.design.previewTextLabel")}>
          <textarea
            value={previewText}
            onChange={(e) => setPreviewText(e.target.value)}
            rows={4}
            placeholder={t("voice.design.previewTextPlaceholder")}
            className={`${field} w-full resize-y`}
          />
        </Field>
        {/* The label asks for 150+; the count turns green once it's there. */}
        <p
          className={`text-right text-xs ${
            previewText.length >= PREVIEW_TEXT_MIN
              ? "text-emerald-400"
              : "text-neutral-500"
          }`}
        >
          {previewText.length}/{PREVIEW_TEXT_MIN}
        </p>
      </div>

      <button
        onClick={generatePreview}
        disabled={busy || !prompt.trim() || !previewText.trim()}
        className={`${btn.solid} w-full gap-2 py-2.5 text-sm font-medium`}
      >
        <Sparkles className={`size-4 ${busy ? "animate-pulse" : ""}`} />
        {busy ? t("voice.design.generating") : t("voice.design.generate")}
      </button>

      {previewUri && (
        <div className="space-y-2">
          {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
          <audio controls src={previewUri} className="w-full" />
          <button
            onClick={confirm}
            disabled={busy}
            className={`${btn.primary} w-full py-2 text-sm font-medium`}
          >
            {busy ? t("voice.clone.cloning") : t("voice.design.accept")}
          </button>
        </div>
      )}

      {error && <p className="text-sm text-red-400">{error}</p>}
    </div>
  );
}
