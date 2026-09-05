import { useEffect, useState } from "react";
import { useT, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btn } from "../lib/ui";
import type { CharacterInput, Language } from "../lib/types";
import VoiceStudio, { type VoiceSelection } from "./VoiceStudio";

const LANGUAGE_LABEL: Record<Language, MessageKey> = {
  zh: "language.zh",
  ja: "language.ja",
  en: "language.en",
  auto: "language.auto",
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
  const t = useT();
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
      setError(t("characterEdit.nameRequired"));
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
    return <p className="p-6 text-sm text-neutral-500">{t("common.loading")}</p>;
  }

  return (
    <div className="mx-auto max-w-2xl space-y-6 p-6 text-neutral-100">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">
          {t(id === "new" ? "characterEdit.newTitle" : "characterEdit.editTitle")}
        </h1>
        <button onClick={onDone} className={`${btn.quiet} px-2 py-1 text-sm`}>
          {t("common.back")}
        </button>
      </div>

      <section className="space-y-3">
        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            {t("characterEdit.name")}
          </label>
          <input
            value={input.name}
            onChange={(e) => set("name", e.target.value)}
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>

        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            {t("characterEdit.language")}
          </label>
          <select
            value={input.language}
            onChange={(e) => set("language", e.target.value as Language)}
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          >
            {(Object.keys(LANGUAGE_LABEL) as Language[]).map((l) => (
              <option key={l} value={l}>
                {t(LANGUAGE_LABEL[l])}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 p-4">
        <label className="block text-xs text-neutral-500">
          {t("characterEdit.polish")}
        </label>
        <div className="flex gap-2">
          <input
            value={polishDesc}
            onChange={(e) => setPolishDesc(e.target.value)}
            placeholder={t("characterEdit.polishPlaceholder")}
            className="flex-1 rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
          <button
            onClick={handlePolish}
            disabled={polishing || !polishDesc.trim()}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {polishing
              ? t("characterEdit.generating")
              : t("characterEdit.generate")}
          </button>
        </div>
      </section>

      <section className="space-y-3">
        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            {t("characterEdit.persona")}
          </label>
          <textarea
            value={input.persona}
            onChange={(e) => set("persona", e.target.value)}
            rows={3}
            className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>
        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            {t("characterEdit.speechHabits")}
          </label>
          <textarea
            value={input.speech_habits}
            onChange={(e) => set("speech_habits", e.target.value)}
            rows={2}
            className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
        </div>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 p-4">
        <label className="block text-xs text-neutral-500">
          {t("characterEdit.voice")}
        </label>
        <div className="flex items-center justify-between">
          <p className="text-sm">
            {input.voice_kind} · {input.voice_id ?? t("common.notSet")}
          </p>
          <button
            onClick={() => setVoiceStudioOpen(true)}
            className={`${btn.outline} px-3 py-1.5 text-sm`}
          >
            {t("characterEdit.openVoiceStudio")}
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
          {t("characterEdit.memoryEnabled")}
        </label>

        <div>
          <label className="mb-1 block text-xs text-neutral-500">
            {t("characterEdit.maxHistoryTurns", {
              count: input.max_history_turns,
            })}
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
          className={`${btn.primary} flex-1 py-2.5 text-sm font-medium`}
        >
          {saving ? t("common.saving") : t("common.save")}
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
