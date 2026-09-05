import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { useT, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btn, interactive } from "../lib/ui";
import type { Memory } from "../lib/types";

const KIND_LABEL: Record<Memory["kind"], MessageKey> = {
  summary: "memory.kind.summary",
  fact: "memory.kind.fact",
  profile: "memory.kind.profile",
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
  const t = useT();
  const [memories, setMemories] = useState<Memory[]>([]);
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
    const rows = await ipc.listMemories(characterId);
    setMemories(rows);
    // Rows that are gone cannot stay ticked, and this is the only place they
    // leave the selection — a delete just refreshes and lets the prune run.
    setSelected((prev) => {
      const alive = new Set(rows.map((m) => m.id));
      return new Set([...prev].filter((id) => alive.has(id)));
    });
    setLoading(false);
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
    await ipc.updateMemory(id, draft);
    setEditingId(null);
    await refresh();
  }

  async function handleConfirmed() {
    if (!pending) return;
    setWorking(true);
    try {
      if (pending === "selected") {
        await ipc.deleteMemories([...selected]);
      } else {
        await ipc.deleteMemory(pending.id);
      }
      setPending(null);
      await refresh();
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6 text-neutral-100">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">
            {t("memory.title", { name: characterName })}
          </h1>
          <p className="text-sm text-neutral-500">
            {t("memory.count", { count: memories.length })}
            {selected.size > 0 &&
              ` · ${t("memory.selected", { count: selected.size })}`}
          </p>
        </div>
        <div className="flex gap-2">
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
            className={`${btn.dangerOutline} px-3 py-1.5 text-sm`}
          >
            {t("memory.deleteSelected")}
          </button>
          <button onClick={onBack} className={`${btn.quiet} px-2 py-1 text-sm`}>
            {t("common.back")}
          </button>
        </div>
      </div>

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
                <span className="rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-400">
                  {t(KIND_LABEL[m.kind])}
                </span>
                {editingId === m.id ? (
                  <div
                    onClick={(e) => e.stopPropagation()}
                    className="flex gap-0.5"
                  >
                    <button
                      onClick={() => saveEdit(m.id)}
                      className={`${btn.accentGhost} px-2 py-1 text-xs`}
                    >
                      {t("common.save")}
                    </button>
                    <button
                      onClick={() => setEditingId(null)}
                      className={`${btn.ghost} px-2 py-1 text-xs`}
                    >
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
                      className={`${btn.ghost} px-2 py-1 text-xs`}
                    >
                      {t("common.edit")}
                    </button>
                    <button
                      onClick={() => setPending(m)}
                      className={`${btn.dangerGhost} px-2 py-1 text-xs`}
                    >
                      {t("common.delete")}
                    </button>
                  </div>
                )}
              </div>
              {editingId === m.id ? (
                <textarea
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onClick={(e) => e.stopPropagation()}
                  rows={3}
                  className="w-full resize-none rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
                />
              ) : (
                <p className="text-sm text-neutral-200">{m.content}</p>
              )}
              <p className="mt-1 text-xs text-neutral-600">{m.updated_at}</p>
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
