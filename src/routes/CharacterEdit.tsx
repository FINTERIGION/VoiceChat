import { useEffect, useState } from "react";
import { ipc } from "../lib/ipc";
import type { CharacterInput, Language } from "../lib/types";
import VoiceStudio, { type VoiceSelection } from "./VoiceStudio";

const LANGUAGE_LABEL: Record<Language, string> = {
  zh: "中文",
  ja: "日本語",
  en: "English",
  auto: "跟随用户",
};

function emptyInput(): CharacterInput {
  return {
    name: "",
    avatar_path: null,
    language: "auto",
    persona: "",
    speech_habits: "",
    voice_kind: "preset",
    voice_id: "longanqian",
    voice_prompt: null,
    memory_enabled: true,
    max_history_turns: 20,
  };
}

export default function CharacterEdit({
  id,
  onDone,
}: {
  id: string | "new";
  onDone: () => void;
}) {
  const [input, setInput] = useState<CharacterInput>(emptyInput());
  const [loading, setLoading] = useState(id !== "new");
  const [saving, setSaving] = useState(false);
  const [voiceStudioOpen, setVoiceStudioOpen] = useState(false);
  const [polishDesc, setPolishDesc] = useState("");
  const [polishing, setPolishing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (id === "new") return;
    ipc
      .listCharacters()
      .then((all) => all.find((c) => c.id === id))
      .then((c) => {
        if (c) {
          setInput({
            name: c.name,
            avatar_path: c.avatar_path,
            language: c.language,
            persona: c.persona,
            speech_habits: c.speech_habits,
            voice_kind: c.voice_kind,
            voice_id: c.voice_id,
            voice_prompt: c.voice_prompt,
            memory_enabled: c.memory_enabled,
            max_history_turns: c.max_history_turns,
          });
        }
        setLoading(false);
      });
  }, [id]);

  function set<K extends keyof CharacterInput>(key: K, value: CharacterInput[K]) {
    setInput((prev) => ({ ...prev, [key]: value }));
  }

  async function handleSave() {
    if (!input.name.trim()) {
      setError("请填写角色名字");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      if (id === "new") {
        await ipc.createCharacter(input);
      } else {
        await ipc.updateCharacter(id, input);
      }
      onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function handlePolish() {
    if (!polishDesc.trim()) return;
    setPolishing(true);
    setError(null);
    try {
      const result = await ipc.polishPersona(polishDesc);
      set("persona", result.persona);
      set("speech_habits", result.speech_habits);
    } catch (e) {
      setError(String(e));
    } finally {
      setPolishing(false);
    }
  }

  function handleVoiceSelect(sel: VoiceSelection) {
    set("voice_kind", sel.voice_kind);
    set("voice_id", sel.voice_id);
    set("voice_prompt", sel.voice_prompt);
    setVoiceStudioOpen(false);
  }

  if (loading) {
    return <p className="p-6 text-sm text-neutral-500">加载中…</p>;
  }

  return (
    <div className="mx-auto max-w-2xl space-y-6 p-6 text-neutral-100">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">
          {id === "new" ? "新建角色" : "编辑角色"}
        </h1>
        <button onClick={onDone} className="text-sm text-neutral-500 hover:text-neutral-300">
          返回
        </button>
      </div>

      <section className="space-y-3">
        <div>
          <label className="mb-1 block text-xs text-neutral-500">名字</label>
          <input
            value={input.name}
            onChange={(e) => set("name", e.target.value)}
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>

        <div>
          <label className="mb-1 block text-xs text-neutral-500">语言</label>
          <select
            value={input.language}
            onChange={(e) => set("language", e.target.value as Language)}
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          >
            {(Object.keys(LANGUAGE_LABEL) as Language[]).map((l) => (
              <option key={l} value={l}>
                {LANGUAGE_LABEL[l]}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 p-4">
        <label className="block text-xs text-neutral-500">AI 润色人设</label>
        <div className="flex gap-2">
          <input
            value={polishDesc}
            onChange={(e) => setPolishDesc(e.target.value)}
            placeholder="用一句话描述这个角色…"
            className="flex-1 rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
          <button
            onClick={handlePolish}
            disabled={polishing || !polishDesc.trim()}
            className="rounded border border-neutral-700 px-3 py-2 text-sm text-neutral-200 disabled:opacity-40"
          >
            {polishing ? "生成中…" : "生成"}
          </button>
        </div>
      </section>

      <section className="space-y-3">
        <div>
          <label className="mb-1 block text-xs text-neutral-500">人设</label>
          <textarea
            value={input.persona}
            onChange={(e) => set("persona", e.target.value)}
            rows={3}
            className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>
        <div>
          <label className="mb-1 block text-xs text-neutral-500">语言习惯</label>
          <textarea
            value={input.speech_habits}
            onChange={(e) => set("speech_habits", e.target.value)}
            rows={2}
            className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 p-4">
        <label className="block text-xs text-neutral-500">音色</label>
        <div className="flex items-center justify-between">
          <p className="text-sm">
            {input.voice_kind} · {input.voice_id ?? "未设置"}
          </p>
          <button
            onClick={() => setVoiceStudioOpen(true)}
            className="rounded border border-neutral-700 px-3 py-1.5 text-sm text-neutral-200"
          >
            打开音色工作室
          </button>
        </div>
      </section>

      <section className="space-y-3 rounded-xl border border-neutral-800 p-4">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={input.memory_enabled}
            onChange={(e) => set("memory_enabled", e.target.checked)}
          />
          启用长期记忆
        </label>

        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            最大历史轮数 ({input.max_history_turns})
          </label>
          <input
            type="range"
            min={5}
            max={50}
            step={1}
            value={input.max_history_turns}
            onChange={(e) => set("max_history_turns", Number(e.target.value))}
            className="w-full"
          />
        </div>
      </section>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="flex gap-2">
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex-1 rounded-lg bg-neutral-100 py-2.5 text-sm font-medium text-neutral-900 disabled:opacity-50"
        >
          {saving ? "保存中…" : "保存"}
        </button>
      </div>

      {voiceStudioOpen && (
        <VoiceStudio
          characterName={input.name || "character"}
          onSelect={handleVoiceSelect}
          onClose={() => setVoiceStudioOpen(false)}
        />
      )}
    </div>
  );
}
