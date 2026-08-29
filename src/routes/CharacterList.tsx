import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { ipc } from "../lib/ipc";
import type { Character } from "../lib/types";

export default function CharacterList({
  onEdit,
  onViewMemory,
}: {
  onEdit: (id: string | "new") => void;
  onViewMemory: (id: string, name: string) => void;
}) {
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
    return <p className="p-6 text-sm text-neutral-500">加载中…</p>;
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6 text-neutral-100">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">角色</h1>
        <button
          onClick={() => onEdit("new")}
          className="rounded bg-neutral-100 px-3 py-1.5 text-sm font-medium text-neutral-900"
        >
          + 新建角色
        </button>
      </div>

      <div className="space-y-3">
        {characters.map((c) => {
          const isCurrent = c.id === currentId;
          return (
            <div
              key={c.id}
              className={`rounded-xl border p-4 ${
                isCurrent
                  ? "border-emerald-500 bg-neutral-900"
                  : "border-neutral-800 bg-neutral-950"
              }`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate font-medium">{c.name}</span>
                    {isCurrent && (
                      <span className="rounded-full bg-emerald-500/20 px-2 py-0.5 text-xs text-emerald-400">
                        当前
                      </span>
                    )}
                  </div>
                  <p className="mt-1 line-clamp-2 text-sm text-neutral-400">
                    {c.persona || "（未填写人设）"}
                  </p>
                  <p className="mt-1 text-xs text-neutral-500">
                    {c.language} · {c.voice_kind} · {c.voice_id ?? "未设置音色"}
                  </p>
                </div>
                <div className="flex shrink-0 flex-col gap-1.5">
                  {!isCurrent && (
                    <button
                      onClick={() => handleSwitch(c.id)}
                      disabled={switching === c.id}
                      className="rounded border border-neutral-700 px-2.5 py-1 text-xs text-neutral-200 disabled:opacity-50"
                    >
                      {switching === c.id ? "切换中…" : "切换"}
                    </button>
                  )}
                  <button
                    onClick={() => onEdit(c.id)}
                    className="rounded border border-neutral-700 px-2.5 py-1 text-xs text-neutral-200"
                  >
                    编辑
                  </button>
                  <button
                    onClick={() => onViewMemory(c.id, c.name)}
                    className="rounded border border-neutral-700 px-2.5 py-1 text-xs text-neutral-200"
                  >
                    记忆
                  </button>
                  <button
                    onClick={() => setPendingDelete(c)}
                    disabled={characters.length <= 1}
                    className="rounded border border-neutral-800 px-2.5 py-1 text-xs text-red-400 disabled:opacity-30"
                  >
                    删除
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {pendingDelete && (
        <ConfirmDialog
          title={`删除角色「${pendingDelete.name}」？`}
          body="该角色的全部对话记录和长期记忆会一并删除，无法恢复。"
          busy={deleting}
          onConfirm={handleDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}
