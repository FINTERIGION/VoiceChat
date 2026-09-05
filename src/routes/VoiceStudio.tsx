import { useCallback, useEffect, useState, type ChangeEvent } from "react";
import { useT, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import type { ManagedVoice, VoiceKind } from "../lib/types";
import { btn, btnBase, cardBtn, hoverGlow, tabBtn } from "../lib/ui";

export interface VoiceSelection {
  voice_kind: VoiceKind;
  voice_id: string;
  voice_prompt: string | null;
}

type Tab = "preset" | "cloud" | "clone" | "design";

export default function VoiceStudio({
  characterName,
  onSelect,
  onClose,
}: {
  characterName: string;
  onSelect: (sel: VoiceSelection) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [tab, setTab] = useState<Tab>("preset");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="flex max-h-full w-full max-w-xl flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100">
        <div className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
          <h2 className="text-sm font-medium">{t("voice.title")}</h2>
          <button
            onClick={onClose}
            className={`${btn.quiet} size-7 text-sm hover:rotate-90`}
          >
            ✕
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
          {tab === "preset" && <PresetTab onSelect={onSelect} />}
          {tab === "cloud" && <CloudTab onSelect={onSelect} />}
          {tab === "clone" && (
            <CloneTab characterName={characterName} onSelect={onSelect} />
          )}
          {tab === "design" && (
            <DesignTab characterName={characterName} onSelect={onSelect} />
          )}
        </div>
      </div>
    </div>
  );
}

const PRESET_VOICE_INFO: Record<
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

function PresetTab({
  onSelect,
}: {
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
        return (
          <button
            key={v}
            onClick={() => onSelect({ voice_kind: "preset", voice_id: v, voice_prompt: null })}
            className={`${cardBtn} px-4 py-2.5 text-sm`}
          >
            <div className="flex items-baseline justify-between gap-3">
              <span className="font-medium">
                {info ? t(info.name) : v}
              </span>
              <span className="text-xs text-neutral-600">{v}</span>
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
function CloudTab({ onSelect }: { onSelect: (sel: VoiceSelection) => void }) {
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
          className={`${btn.quiet} shrink-0 px-2 py-1 text-xs`}
        >
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
        <CloudVoiceRow key={v.voice_id} voice={v} onSelect={onSelect} />
      ))}
    </div>
  );
}

function CloudVoiceRow({
  voice,
  onSelect,
}: {
  voice: ManagedVoice;
  onSelect: (sel: VoiceSelection) => void;
}) {
  const t = useT();
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
      className={`${cardBtn} px-4 py-2.5 text-sm`}
    >
      <p className="truncate font-mono text-xs text-neutral-300">
        {voice.voice_id}
      </p>
      <p className="mt-1 flex flex-wrap items-center gap-2 text-xs text-neutral-500">
        {voice.created_at && <span>{voice.created_at}</span>}
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
      try {
        const uri = await ipc.stopRecording();
        setDataUri(uri);
      } catch (e) {
        setError(String(e));
      } finally {
        setRecording(false);
      }
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
          className={`${btnBase} ${hoverGlow} w-full rounded-lg py-2.5 text-sm font-medium ${
            recording
              ? "bg-red-500 text-neutral-950 hover:bg-red-400 hover:shadow-red-500/25"
              : "bg-neutral-800 text-neutral-100 hover:bg-neutral-700 hover:shadow-black/40"
          }`}
        >
          {t(recording ? "voice.clone.stop" : "voice.clone.start")}
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
        <label className={`${btn.outline} w-full px-3 py-2 text-sm`}>
          {fileName ?? t("voice.clone.chooseFile")}
          <input
            type="file"
            accept="audio/*"
            onChange={handleFileChange}
            className="hidden"
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
        <label className="text-xs text-neutral-500">
          {t("voice.design.promptLabel")}
        </label>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value.slice(0, 500))}
          rows={3}
          placeholder={t("voice.design.promptPlaceholder")}
          className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        />
        <p className="text-right text-xs text-neutral-600">{prompt.length}/500</p>
      </div>

      <div className="space-y-1.5">
        <label className="text-xs text-neutral-500">
          {t("voice.design.previewTextLabel")}
        </label>
        <textarea
          value={previewText}
          onChange={(e) => setPreviewText(e.target.value)}
          rows={4}
          placeholder={t("voice.design.previewTextPlaceholder")}
          className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        />
      </div>

      <button
        onClick={generatePreview}
        disabled={busy || !prompt.trim() || !previewText.trim()}
        className={`${btn.solid} w-full py-2.5 text-sm font-medium`}
      >
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
