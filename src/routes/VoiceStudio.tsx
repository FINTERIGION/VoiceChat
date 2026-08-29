import { useEffect, useState, type ChangeEvent } from "react";
import { ipc } from "../lib/ipc";
import type { VoiceKind } from "../lib/types";

export interface VoiceSelection {
  voice_kind: VoiceKind;
  voice_id: string;
  voice_prompt: string | null;
}

type Tab = "preset" | "clone" | "design";

export default function VoiceStudio({
  characterName,
  onSelect,
  onClose,
}: {
  characterName: string;
  onSelect: (sel: VoiceSelection) => void;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<Tab>("preset");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="flex max-h-full w-full max-w-xl flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100">
        <div className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
          <h2 className="text-sm font-medium">音色工作室</h2>
          <button
            onClick={onClose}
            className="text-neutral-500 hover:text-neutral-300"
          >
            ✕
          </button>
        </div>

        <nav className="flex gap-1 border-b border-neutral-800 px-5 pt-2">
          {(
            [
              ["preset", "预置"],
              ["clone", "录音复刻"],
              ["design", "文本设计"],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              onClick={() => setTab(key)}
              className={`rounded-t-lg px-3 py-1.5 text-sm ${
                tab === key
                  ? "bg-neutral-900 text-neutral-100"
                  : "text-neutral-500 hover:text-neutral-300"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>

        <div className="flex-1 overflow-y-auto p-5">
          {tab === "preset" && <PresetTab onSelect={onSelect} />}
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

const PRESET_VOICE_INFO: Record<string, { name: string; desc: string; lang: string }> = {
  longanqian: { name: "龙安千", desc: "默认音色", lang: "语言未公开说明，推测与同系列一致（中/英）" },
  longanlingxin: { name: "龙安灵心", desc: "女声・知心温暖音", lang: "中文（普通话）、英文" },
  longanlingxi: { name: "龙安灵希", desc: "女声・可爱甜美音", lang: "中文（普通话）、英文" },
  longanxiaoxin: { name: "龙安小昕", desc: "女声・亲切活泼音", lang: "中文（普通话）、英文" },
  longanlufeng: { name: "龙安鲁风", desc: "男声・明亮开朗音", lang: "中文（普通话）、英文" },
};

function PresetTab({
  onSelect,
}: {
  onSelect: (sel: VoiceSelection) => void;
}) {
  const [voices, setVoices] = useState<string[]>([]);

  useEffect(() => {
    ipc.listPresetVoices().then(setVoices);
  }, []);

  return (
    <div className="space-y-2">
      <p className="text-sm text-neutral-500">
        预置音色暂无线上试听样本，按名称选择。
      </p>
      {voices.map((v) => {
        const info = PRESET_VOICE_INFO[v];
        return (
          <button
            key={v}
            onClick={() => onSelect({ voice_kind: "preset", voice_id: v, voice_prompt: null })}
            className="block w-full rounded-lg border border-neutral-800 px-4 py-2.5 text-left text-sm hover:border-neutral-600"
          >
            <div className="flex items-baseline justify-between gap-3">
              <span className="font-medium">{info?.name ?? v}</span>
              <span className="text-xs text-neutral-600">{v}</span>
            </div>
            {info && (
              <p className="mt-0.5 text-xs text-neutral-500">
                {info.desc}
                <span className="text-neutral-600"> ・ {info.lang}</span>
              </p>
            )}
          </button>
        );
      })}
    </div>
  );
}

function CloneTab({
  characterName,
  onSelect,
}: {
  characterName: string;
  onSelect: (sel: VoiceSelection) => void;
}) {
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
    reader.onerror = () => setError("读取文件失败");
    reader.readAsDataURL(file);
  }

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <p className="text-sm text-neutral-400">
          应用内录制 10–20 秒清晰语音（上限 60 秒）。
        </p>
        <button
          onClick={toggleRecording}
          className={`w-full rounded-lg py-2.5 text-sm font-medium ${
            recording
              ? "bg-red-500 text-neutral-950"
              : "bg-neutral-800 text-neutral-100"
          }`}
        >
          {recording ? "■ 停止录音" : "● 开始录音"}
        </button>
        {dataUri && (
          <div className="space-y-2">
            {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
            <audio controls src={dataUri} className="w-full" />
            <button
              onClick={() => cloneFrom(dataUri)}
              disabled={busy}
              className="w-full rounded-lg bg-neutral-100 py-2 text-sm font-medium text-neutral-900 disabled:opacity-50"
            >
              {busy ? "复刻中…" : "使用此录音克隆"}
            </button>
          </div>
        )}
      </div>

      <div className="space-y-2 border-t border-neutral-800 pt-4">
        <p className="text-sm text-neutral-400">
          或选择一段本地音频文件（10–20 秒清晰语音）。
        </p>
        <label className="block w-full cursor-pointer rounded-lg border border-neutral-700 px-3 py-2 text-center text-sm text-neutral-200 hover:border-neutral-500">
          {fileName ?? "选择文件…"}
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
              className="w-full rounded-lg border border-neutral-700 py-2 text-sm text-neutral-200 disabled:opacity-40"
            >
              {busy ? "复刻中…" : "使用此文件克隆"}
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
          音色描述（仅支持中/英文，≤500 字）
        </label>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value.slice(0, 500))}
          rows={3}
          placeholder="例如：温柔知性的女声，语速偏慢，略带鼻音"
          className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        />
        <p className="text-right text-xs text-neutral-600">{prompt.length}/500</p>
      </div>

      <div className="space-y-1.5">
        <label className="text-xs text-neutral-500">
          试听文本（建议 150 字以上，够念满 15 秒）
        </label>
        <textarea
          value={previewText}
          onChange={(e) => setPreviewText(e.target.value)}
          rows={4}
          placeholder="用来生成试听样本的一段文字…"
          className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        />
      </div>

      <button
        onClick={generatePreview}
        disabled={busy || !prompt.trim() || !previewText.trim()}
        className="w-full rounded-lg bg-neutral-800 py-2.5 text-sm font-medium text-neutral-100 disabled:opacity-50"
      >
        {busy ? "生成中…" : "生成试听"}
      </button>

      {previewUri && (
        <div className="space-y-2">
          {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
          <audio controls src={previewUri} className="w-full" />
          <button
            onClick={confirm}
            disabled={busy}
            className="w-full rounded-lg bg-neutral-100 py-2 text-sm font-medium text-neutral-900 disabled:opacity-50"
          >
            {busy ? "复刻中…" : "满意，使用此音色"}
          </button>
        </div>
      )}

      {error && <p className="text-sm text-red-400">{error}</p>}
    </div>
  );
}
