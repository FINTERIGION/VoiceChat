import { useEffect, useId, useState } from "react";
import { ArrowLeft, AudioLines, Camera, Undo2, WandSparkles } from "lucide-react";
import Avatar from "../components/Avatar";
import AvatarDialog from "../components/AvatarDialog";
import ConfirmDialog from "../components/ConfirmDialog";
import Field from "../components/Field";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { describeVoice, LANGUAGE_LABEL } from "../lib/labels";
import { btn, field, interactive } from "../lib/ui";
import type { CharacterInput, Language } from "../lib/types";
import VoiceStudio, { type VoiceSelection } from "./VoiceStudio";

/**
 * Longest name allowed, in display columns — Chinese characters count two —
 * so 12 Chinese characters or 24 Latin letters. Mirrors
 * `character::NAME_MAX_WIDTH`, which is what actually enforces it.
 *
 * Checked on save rather than stopped at the keyboard: cutting the text
 * off as it is typed would also cut into pinyin still being composed.
 */
const NAME_MAX_WIDTH = 24;

/** Mirrors `character::is_wide` on the Rust side. */
function isWide(code: number): boolean {
  return (
    (code >= 0x1100 && code <= 0x115f) ||
    (code >= 0x2e80 && code <= 0x303e) ||
    (code >= 0x3041 && code <= 0x33ff) ||
    (code >= 0x3400 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0x9fff) ||
    (code >= 0xa000 && code <= 0xa4cf) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe30 && code <= 0xfe4f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f300 && code <= 0x1f64f) ||
    (code >= 0x1f900 && code <= 0x1f9ff) ||
    (code >= 0x20000 && code <= 0x3fffd)
  );
}

function nameWidth(name: string): number {
  let width = 0;
  for (const ch of name) width += isWide(ch.codePointAt(0)!) ? 2 : 1;
  return width;
}

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
  onDirtyChange,
}: {
  id: string | "new";
  /** `savedId` is the character just saved; absent when backing out. */
  onDone: (savedId?: string) => void;
  /**
   * Whether the form holds edits that haven't been saved — so the tab bar,
   * which can take this page away without going through its back button,
   * knows to ask first.
   */
  onDirtyChange: (dirty: boolean) => void;
}) {
  const t = useT();
  const [input, setInput] = useState<CharacterInput>(emptyInput());
  // The form as loaded, to tell an edit from a look around.
  const [saved, setSaved] = useState<CharacterInput>(emptyInput());
  const [loading, setLoading] = useState(id !== "new");
  const [saving, setSaving] = useState(false);
  const [voiceStudioOpen, setVoiceStudioOpen] = useState(false);
  const [avatarDialogOpen, setAvatarDialogOpen] = useState(false);
  const [polishDesc, setPolishDesc] = useState("");
  const [polishing, setPolishing] = useState(false);
  // What the persona fields held before the AI replaced them, while that
  // replacement can still be taken back. Cleared as soon as either field is
  // typed into: undoing then would throw away the typing too.
  const [beforePolish, setBeforePolish] = useState<Pick<
    CharacterInput,
    "persona" | "speech_habits"
  > | null>(null);
  const [confirmingDiscard, setConfirmingDiscard] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const polishId = useId();

  const dirty = JSON.stringify(input) !== JSON.stringify(saved);
  const nameTooLong = nameWidth(input.name.trim()) > NAME_MAX_WIDTH;

  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty, onDirtyChange]);

  useEffect(() => {
    if (id === "new") return;
    ipc
      .listCharacters()
      .then((all) => all.find((c) => c.id === id))
      .then((c) => {
        if (c) {
          const loaded: CharacterInput = {
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
          };
          setInput(loaded);
          setSaved(loaded);
        }
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [id]);

  function set<K extends keyof CharacterInput>(key: K, value: CharacterInput[K]) {
    setInput((prev) => ({ ...prev, [key]: value }));
  }

  function handleBack() {
    if (dirty) {
      setConfirmingDiscard(true);
    } else {
      onDone();
    }
  }

  async function handleSave() {
    if (!input.name.trim()) {
      setError(t("characterEdit.nameRequired"));
      return;
    }
    if (nameTooLong) {
      setError(t("characterEdit.nameTooLong"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const saved =
        id === "new"
          ? await ipc.createCharacter(input)
          : await ipc.updateCharacter(id, input);
      onDone(saved.id);
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
      // Nothing to take back if both fields were still empty.
      setBeforePolish(
        input.persona.trim() || input.speech_habits.trim()
          ? { persona: input.persona, speech_habits: input.speech_habits }
          : null,
      );
      set("persona", result.persona);
      set("speech_habits", result.speech_habits);
    } catch (e) {
      setError(String(e));
    } finally {
      setPolishing(false);
    }
  }

  function undoPolish() {
    if (!beforePolish) return;
    set("persona", beforePolish.persona);
    set("speech_habits", beforePolish.speech_habits);
    setBeforePolish(null);
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
      <div className="flex items-center gap-2">
        <button
          onClick={handleBack}
          title={t("common.back")}
          aria-label={t("common.back")}
          className={`${btn.quiet} -ml-2 size-8`}
        >
          <ArrowLeft className="size-5" />
        </button>
        <h1 className="text-xl font-semibold">
          {t(id === "new" ? "characterEdit.newTitle" : "characterEdit.editTitle")}
        </h1>
      </div>

      <section className="flex items-start gap-5">
        <div className="flex shrink-0 flex-col items-center gap-1.5">
          <button
            onClick={() => setAvatarDialogOpen(true)}
            title={t(input.avatar_path ? "avatar.change" : "avatar.set")}
            aria-label={t(input.avatar_path ? "avatar.change" : "avatar.set")}
            className={`${interactive} group relative rounded-full`}
          >
            <Avatar
              id={id === "new" ? "" : id}
              name={input.name}
              avatarPath={input.avatar_path}
              size="lg"
            />
            <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/55 text-neutral-100 opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100">
              <Camera className="size-4" />
            </span>
          </button>
          {input.avatar_path ? (
            <button
              onClick={() => set("avatar_path", null)}
              className={`${btn.quiet} px-1 text-xs`}
            >
              {t("avatar.remove")}
            </button>
          ) : (
            <span className="text-xs text-neutral-500">
              {t("avatar.label")}
            </span>
          )}
        </div>
        <div className="grid min-w-0 flex-1 grid-cols-2 gap-3">
          <Field
            label={
              <span className="flex justify-between gap-2">
                {t("characterEdit.name")}
                <span
                  title={t("characterEdit.nameWidthTitle")}
                  className={`tabular-nums ${nameTooLong ? "text-red-400" : "text-neutral-500"}`}
                >
                  {nameWidth(input.name.trim())}/{NAME_MAX_WIDTH}
                </span>
              </span>
            }
            hint={
              nameTooLong && (
                <span className="text-red-400">
                  {t("characterEdit.nameTooLong")}
                </span>
              )
            }
          >
            <input
              value={input.name}
              onChange={(e) => set("name", e.target.value)}
              aria-invalid={nameTooLong}
              className={`${field} w-full ${nameTooLong ? "border-red-500/60" : ""}`}
            />
          </Field>
          <Field label={t("characterEdit.language")}>
            <select
              value={input.language}
              onChange={(e) => set("language", e.target.value as Language)}
              className={`${field} w-full`}
            >
              {(Object.keys(LANGUAGE_LABEL) as Language[]).map((l) => (
                <option key={l} value={l}>
                  {t(LANGUAGE_LABEL[l])}
                </option>
              ))}
            </select>
          </Field>
        </div>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 bg-neutral-900/40 p-4">
        {/* Not a <Field>: the button beside the input would be activated by
            clicks on the caption too. */}
        <label htmlFor={polishId} className="block text-xs text-neutral-400">
          {t("characterEdit.polish")}
        </label>
        <div className="flex gap-2">
          <input
            id={polishId}
            value={polishDesc}
            onChange={(e) => setPolishDesc(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !polishing) void handlePolish();
            }}
            placeholder={t("characterEdit.polishPlaceholder")}
            className={`${field} min-w-0 flex-1`}
          />
          <button
            onClick={handlePolish}
            disabled={polishing || !polishDesc.trim()}
            className={`${btn.outline} shrink-0 gap-1.5 px-3 py-2 text-sm`}
          >
            <WandSparkles
              className={`size-4 ${polishing ? "animate-pulse" : ""}`}
            />
            {polishing
              ? t("characterEdit.generating")
              : t("characterEdit.generate")}
          </button>
        </div>
        {beforePolish && (
          <p className="flex items-center gap-2 text-xs text-neutral-500">
            {t("characterEdit.polished")}
            <button
              onClick={undoPolish}
              className={`${btn.accentGhost} gap-1 px-1.5 py-0.5 text-xs`}
            >
              <Undo2 className="size-3.5" />
              {t("characterEdit.undoPolish")}
            </button>
          </p>
        )}
      </section>

      <section className="space-y-3">
        <Field label={t("characterEdit.persona")}>
          <textarea
            value={input.persona}
            onChange={(e) => {
              setBeforePolish(null);
              set("persona", e.target.value);
            }}
            rows={6}
            className={`${field} w-full resize-y`}
          />
        </Field>
        <Field label={t("characterEdit.speechHabits")}>
          <textarea
            value={input.speech_habits}
            onChange={(e) => {
              setBeforePolish(null);
              set("speech_habits", e.target.value);
            }}
            rows={3}
            className={`${field} w-full resize-y`}
          />
        </Field>
      </section>

      <section className="space-y-2 rounded-xl border border-neutral-800 bg-neutral-900/40 p-4">
        <p className="text-xs text-neutral-400">{t("characterEdit.voice")}</p>
        <div className="flex items-center justify-between gap-3">
          <p className="flex min-w-0 items-center gap-2 text-sm">
            <AudioLines className="size-4 shrink-0 text-neutral-500" />
            <span className="truncate">
              {describeVoice(t, input.voice_kind, input.voice_id)}
            </span>
          </p>
          <button
            onClick={() => setVoiceStudioOpen(true)}
            className={`${btn.outline} shrink-0 px-3 py-1.5 text-sm`}
          >
            {t("characterEdit.openVoiceStudio")}
          </button>
        </div>
      </section>

      <section className="space-y-4 rounded-xl border border-neutral-800 bg-neutral-900/40 p-4">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={input.memory_enabled}
            onChange={(e) => set("memory_enabled", e.target.checked)}
          />
          {t("characterEdit.memoryEnabled")}
        </label>
        <Field
          label={t("characterEdit.maxHistoryTurns", {
            count: input.max_history_turns,
          })}
        >
          <input
            type="range"
            min={5}
            max={50}
            step={1}
            value={input.max_history_turns}
            onChange={(e) => set("max_history_turns", Number(e.target.value))}
          />
        </Field>
      </section>

      {error && (
        <p role="alert" className="text-sm text-red-400">
          {error}
        </p>
      )}

      <div className="flex gap-2">
        <button
          onClick={handleSave}
          disabled={saving}
          className={`${btn.primary} flex-1 py-2.5 text-sm font-medium`}
        >
          {saving ? t("common.saving") : t("common.save")}
        </button>
      </div>

      {avatarDialogOpen && (
        <AvatarDialog
          characterName={input.name}
          persona={input.persona}
          onSaved={(path) => {
            set("avatar_path", path);
            setAvatarDialogOpen(false);
          }}
          onClose={() => setAvatarDialogOpen(false)}
        />
      )}

      {voiceStudioOpen && (
        <VoiceStudio
          characterName={input.name || "character"}
          currentVoiceId={input.voice_id}
          onSelect={handleVoiceSelect}
          onClose={() => setVoiceStudioOpen(false)}
        />
      )}

      {confirmingDiscard && (
        <ConfirmDialog
          title={t("characterEdit.discardTitle")}
          body={t("characterEdit.discardBody")}
          confirmLabel={t("characterEdit.discardConfirm")}
          onConfirm={() => onDone()}
          onCancel={() => setConfirmingDiscard(false)}
        />
      )}
    </div>
  );
}
