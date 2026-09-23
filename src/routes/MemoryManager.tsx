import { useEffect, useState } from "react";
import { ArrowLeft, Check, Pencil, Trash2, X } from "lucide-react";
import ConfirmDialog from "../components/ConfirmDialog";
import { formatDateTime } from "../lib/format";
import { useI18n, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btn, field, interactive } from "../lib/ui";
import type { Memory } from "../lib/types";

const KIND_LABEL: Record<Memory["kind"], MessageKey> = {
  summary: "memory.kind.summary",
  fact: "memory.kind.fact",
  profile: "memory.kind.profile",
  open_loop: "memory.kind.openLoop",
};

export default function MemoryManager({
  characterId,
  characterName,
  onBack,
}: {
  characterId: string;
  characterName: string;
  onBack: () => void;
}) {
  const { t, lang } = useI18n();
  const [memories, setMemories] = useState<Memory[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // Either a single memory awaiting confirmation, or the bulk delete of
  // everything currently ticked.
  const [pending, setPending] = useState<Memory | "selected" | null>(null);
  const [working, setWorking] = useState(false);

  async function refresh() {
    setLoading(true);
    try {
      const rows = await ipc.listMemories(characterId);
      setMemories(rows);
      // Rows that are gone cannot stay ticked, and this is the only place
      // they leave the selection — a delete just refreshes and lets the
      // prune run.
      setSelected((prev) => {
        const alive = new Set(rows.map((m) => m.id));
        return new Set([...prev].filter((id) => alive.has(id)));
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [characterId]);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  const allSelected = memories.length > 0 && selected.size === memories.length;

  function toggleAll() {
    setSelected(allSelected ? new Set() : new Set(memories.map((m) => m.id)));
  }

  function startEdit(m: Memory) {
    setEditingId(m.id);
    setDraft(m.content);
  }

  async function saveEdit(id: string) {
    setError(null);
    try {
      await ipc.updateMemory(id, draft);
    } catch (e) {
      // The editor stays open with the draft in it, so nothing typed is lost.
      setError(String(e));
      return;
    }
    setEditingId(null);
    await refresh();
  }

  async function handleConfirmed() {
    if (!pending) return;
    setWorking(true);
    setError(null);
    try {
      if (pending === "selected") {
        await ipc.deleteMemories([...selected]);
      } else {
        await ipc.deleteMemory(pending.id);
      }
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
      setPending(null);
    }
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6 text-neutral-100">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <button
            onClick={onBack}
            title={t("common.back")}
            aria-label={t("common.back")}
            className={`${btn.quiet} -ml-2 size-8 shrink-0`}
          >
            <ArrowLeft className="size-5" />
          </button>
          <div className="min-w-0">
            <h1 className="truncate text-xl font-semibold">
              {t("memory.title", { name: characterName })}
            </h1>
            <p className="text-sm text-neutral-500">
              {t("memory.count", { count: memories.length })}
              {selected.size > 0 &&
                ` · ${t("memory.selected", { count: selected.size })}`}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          <button
            onClick={toggleAll}
            disabled={memories.length === 0}
            className={`${btn.outline} px-3 py-1.5 text-sm`}
          >
            {t(allSelected ? "memory.deselectAll" : "memory.selectAll")}
          </button>
          <button
            onClick={() => setPending("selected")}
            disabled={selected.size === 0}
            className={`${btn.dangerOutline} gap-1.5 px-3 py-1.5 text-sm`}
          >
            <Trash2 className="size-4" />
            {t("memory.deleteSelected")}
          </button>
        </div>
      </div>

      {error && (
        <p
          role="alert"
          className="rounded-lg bg-red-500/10 px-3 py-2 text-sm text-red-400"
        >
          {error}
        </p>
      )}

      {loading ? (
        <p className="text-sm text-neutral-500">{t("common.loading")}</p>
      ) : memories.length === 0 ? (
        <p className="text-sm text-neutral-500">{t("memory.empty")}</p>
      ) : (
        <div className="space-y-3">
          {memories.map((m) => (
            // The card is the tick: clicking anywhere on it that isn't a
            // button or the editor selects it, so nothing has to be aimed at.
            <div
              key={m.id}
              role="checkbox"
              aria-checked={selected.has(m.id)}
              aria-label={t("memory.selectOne")}
              tabIndex={0}
              onClick={() => toggle(m.id)}
              onKeyDown={(e) => {
                // Only the card's own keys — Enter or Space on one of the
                // buttons inside it belongs to that button.
                if (e.target !== e.currentTarget) return;
                if (e.key === " " || e.key === "Enter") {
                  e.preventDefault();
                  toggle(m.id);
                }
              }}
              className={`${interactive} group rounded-xl border p-4 ${
                selected.has(m.id)
                  ? "border-neutral-500 bg-neutral-900"
                  : "border-neutral-800 bg-neutral-950 hover:border-neutral-700 hover:bg-neutral-900/60"
              }`}
            >
              <div className="mb-2 flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  {/* Shows the card's tick state; the card itself is the
                      control, so this box takes no clicks or focus. */}
                  <input
                    type="checkbox"
                    checked={selected.has(m.id)}
                    readOnly
                    tabIndex={-1}
                    aria-hidden
                    className="pointer-events-none"
                  />
                  <span className="rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-400">
                    {t(KIND_LABEL[m.kind])}
                  </span>
                </div>
                {editingId === m.id ? (
                  <div
                    onClick={(e) => e.stopPropagation()}
                    className="flex gap-0.5"
                  >
                    <button
                      onClick={() => saveEdit(m.id)}
                      className={`${btn.accentGhost} gap-1 px-2 py-1 text-xs`}
                    >
                      <Check className="size-3.5" />
                      {t("common.save")}
                    </button>
                    <button
                      onClick={() => setEditingId(null)}
                      className={`${btn.ghost} gap-1 px-2 py-1 text-xs`}
                    >
                      <X className="size-3.5" />
                      {t("common.cancel")}
                    </button>
                  </div>
                ) : (
                  // Idle rows stay uncluttered; the actions fade in with the
                  // cursor, and with keyboard focus so they stay reachable.
                  <div
                    onClick={(e) => e.stopPropagation()}
                    className="flex gap-0.5 opacity-0 transition-opacity duration-200 group-focus-within:opacity-100 group-hover:opacity-100"
                  >
                    <button
                      onClick={() => startEdit(m)}
                      className={`${btn.ghost} gap-1 px-2 py-1 text-xs`}
                    >
                      <Pencil className="size-3.5" />
                      {t("common.edit")}
                    </button>
                    <button
                      onClick={() => setPending(m)}
                      className={`${btn.dangerGhost} gap-1 px-2 py-1 text-xs`}
                    >
                      <Trash2 className="size-3.5" />
                      {t("common.delete")}
                    </button>
                  </div>
                )}
              </div>
              {editingId === m.id ? (
                <textarea
                  value={draft}
                  autoFocus
                  onChange={(e) => setDraft(e.target.value)}
                  onClick={(e) => e.stopPropagation()}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                      e.preventDefault();
                      saveEdit(m.id);
                    } else if (e.key === "Escape") {
                      setEditingId(null);
                    }
                  }}
                  rows={3}
                  aria-label={t(KIND_LABEL[m.kind])}
                  className={`${field} w-full resize-y`}
                />
              ) : (
                <p className="text-sm whitespace-pre-wrap break-words text-neutral-200">
                  {m.content}
                </p>
              )}
              <p className="mt-1 text-xs text-neutral-500">
                {formatDateTime(m.updated_at, lang)}
              </p>
            </div>
          ))}
        </div>
      )}

      {pending && (
        <ConfirmDialog
          title={
            pending === "selected"
              ? t("memory.deleteSelectedTitle", { count: selected.size })
              : t("memory.deleteTitle")
          }
          body={
            pending === "selected"
              ? t("memory.deleteSelectedBody")
              : pending.content
          }
          busy={working}
          onConfirm={handleConfirmed}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
