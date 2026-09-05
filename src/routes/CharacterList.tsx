import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import type { Character } from "../lib/types";
import { btn, hoverLift } from "../lib/ui";

/** A character card is one big button; the shared hover motion says so. */
const CARD =
  "group relative select-none overflow-hidden rounded-xl border p-4 outline-none transition duration-200 focus-visible:ring-2 focus-visible:ring-neutral-500";
const CARD_CURRENT = "border-emerald-500/60 bg-neutral-900 shadow-lg shadow-emerald-500/5";
const CARD_IDLE = `cursor-pointer border-neutral-800 bg-neutral-950 ${hoverLift} hover:border-neutral-700 hover:bg-neutral-900 hover:shadow-black/40`;

export default function CharacterList({
  onEdit,
  onViewMemory,
}: {
  onEdit: (id: string | "new") => void;
  onViewMemory: (id: string, name: string) => void;
}) {
  const t = useT();
  const [characters, setCharacters] = useState<Character[]>([]);
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [switching, setSwitching] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Character | null>(null);
  const [deleting, setDeleting] = useState(false);

  async function refresh() {
    const [list, current] = await Promise.all([
      ipc.listCharacters(),
      ipc.getCurrentCharacterId(),
    ]);
    setCharacters(list);
    setCurrentId(current);
    setLoading(false);
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleSwitch(id: string) {
    if (id === currentId || switching) {
      return;
    }
    setSwitching(id);
    try {
      await ipc.switchCharacter(id);
      setCurrentId(id);
    } finally {
      setSwitching(null);
    }
  }

  async function handleDelete() {
    if (!pendingDelete || characters.length <= 1) {
      return;
    }
    setDeleting(true);
    try {
      await ipc.deleteCharacter(pendingDelete.id);
      setPendingDelete(null);
      await refresh();
    } finally {
      setDeleting(false);
    }
  }

  if (loading) {
    return <p className="p-6 text-sm text-neutral-500">{t("common.loading")}</p>;
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6 text-neutral-100">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">{t("characters.title")}</h1>
        <button
          onClick={() => onEdit("new")}
          className={`${btn.primary} px-3 py-1.5 text-sm font-medium`}
        >
          {t("characters.new")}
        </button>
      </div>

      <ul className="space-y-2.5">
        {characters.map((c) => {
          const isCurrent = c.id === currentId;
          const isSwitching = switching === c.id;
          return (
            // The whole card is the switch affordance: double-click (or Enter,
            // for the keyboard) makes this the current character. The nested
            // buttons stop their own double-clicks from reaching it.
            <li
              key={c.id}
              tabIndex={0}
              aria-current={isCurrent}
              aria-busy={isSwitching}
              title={isCurrent ? undefined : t("characters.switchHint")}
              onDoubleClick={() => handleSwitch(c.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && e.target === e.currentTarget) {
                  handleSwitch(c.id);
                }
              }}
              className={`${CARD} ${isCurrent ? CARD_CURRENT : CARD_IDLE} ${
                isSwitching ? "opacity-60" : ""
              }`}
            >
              {/* Left accent: solid for the current character, wiping in on hover for the rest. */}
              <span
                aria-hidden
                className={`absolute inset-y-0 left-0 w-[3px] transition-transform duration-200 ${
                  isCurrent
                    ? "bg-emerald-400"
                    : "scale-y-0 bg-neutral-600 group-hover:scale-y-100"
                }`}
              />

              <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2">
                  <span className="truncate font-medium">{c.name}</span>
                  {isCurrent && (
                    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-400">
                      <span className="size-1.5 rounded-full bg-emerald-400" />
                      {t("characters.current")}
                    </span>
                  )}
                </div>

                <div
                  onDoubleClick={(e) => e.stopPropagation()}
                  className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity duration-200 group-focus-within:opacity-100 group-hover:opacity-100"
                >
                  <button
                    onClick={() => onEdit(c.id)}
                    className={`${btn.ghost} px-2 py-1 text-xs`}
                  >
                    {t("common.edit")}
                  </button>
                  <button
                    onClick={() => onViewMemory(c.id, c.name)}
                    className={`${btn.ghost} px-2 py-1 text-xs`}
                  >
                    {t("characters.memory")}
                  </button>
                  <button
                    onClick={() => setPendingDelete(c)}
                    disabled={characters.length <= 1}
                    className={`${btn.dangerGhost} px-2 py-1 text-xs`}
                  >
                    {t("common.delete")}
                  </button>
                </div>
              </div>

              <p className="mt-1 line-clamp-2 text-sm text-neutral-400">
                {c.persona || t("characters.noPersona")}
              </p>

              <div className="mt-1.5 flex items-end justify-between gap-3">
                <p className="truncate text-xs text-neutral-500">
                  {c.language} · {c.voice_kind} ·{" "}
                  {c.voice_id ?? t("characters.noVoice")}
                </p>
                {!isCurrent && (
                  <span
                    className={`shrink-0 text-xs transition duration-200 ${
                      isSwitching
                        ? "text-emerald-400"
                        : "translate-x-1 text-neutral-500 opacity-0 group-focus-within:translate-x-0 group-focus-within:opacity-100 group-hover:translate-x-0 group-hover:opacity-100"
                    }`}
                  >
                    {isSwitching
                      ? t("characters.switching")
                      : t("characters.switchHint")}
                  </span>
                )}
              </div>
            </li>
          );
        })}
      </ul>

      {pendingDelete && (
        <ConfirmDialog
          title={t("characters.deleteTitle", { name: pendingDelete.name })}
          body={t("characters.deleteBody")}
          busy={deleting}
          onConfirm={handleDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}
