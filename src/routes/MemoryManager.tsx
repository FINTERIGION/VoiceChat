import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { ipc } from "../lib/ipc";
import type { Memory } from "../lib/types";

const KIND_LABEL: Record<Memory["kind"], string> = {
  summary: "摘要",
  fact: "事实",
  profile: "画像",
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
  const [memories, setMemories] = useState<Memory[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  // Either a single memory awaiting confirmation, or the "clear all" action.
  const [pending, setPending] = useState<Memory | "all" | null>(null);
  const [working, setWorking] = useState(false);

  async function refresh() {
    setLoading(true);
    setMemories(await ipc.listMemories(characterId));
    setLoading(false);
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [characterId]);

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
      if (pending === "all") {
        await ipc.clearMemories(characterId);
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
          <h1 className="text-xl font-semibold">{characterName} 的记忆</h1>
          <p className="text-sm text-neutral-500">
            共 {memories.length} 条
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setPending("all")}
            disabled={memories.length === 0}
            className="rounded border border-neutral-800 px-3 py-1.5 text-sm text-red-400 disabled:opacity-30"
          >
            一键清空
          </button>
          <button
            onClick={onBack}
            className="text-sm text-neutral-500 hover:text-neutral-300"
          >
            返回
          </button>
        </div>
      </div>

      {loading ? (
        <p className="text-sm text-neutral-500">加载中…</p>
      ) : memories.length === 0 ? (
        <p className="text-sm text-neutral-500">
          还没有记忆。开启「启用长期记忆」并聊几轮之后，这里会出现摘要和事实条目。
        </p>
      ) : (
        <div className="space-y-3">
          {memories.map((m) => (
            <div
              key={m.id}
              className="rounded-xl border border-neutral-800 bg-neutral-950 p-4"
            >
              <div className="mb-2 flex items-center justify-between">
                <span className="rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-400">
                  {KIND_LABEL[m.kind]}
                </span>
                <div className="flex gap-2 text-xs">
                  {editingId === m.id ? (
                    <>
                      <button
                        onClick={() => saveEdit(m.id)}
                        className="text-emerald-400"
                      >
                        保存
                      </button>
                      <button
                        onClick={() => setEditingId(null)}
                        className="text-neutral-500"
                      >
                        取消
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        onClick={() => startEdit(m)}
                        className="text-neutral-400 hover:text-neutral-200"
                      >
                        编辑
                      </button>
                      <button
                        onClick={() => setPending(m)}
                        className="text-red-400"
                      >
                        删除
                      </button>
                    </>
                  )}
                </div>
              </div>
              {editingId === m.id ? (
                <textarea
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
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
            pending === "all"
              ? `清空「${characterName}」的全部记忆？`
              : "删除这条记忆？"
          }
          body={
            pending === "all"
              ? `${memories.length} 条记忆会被全部删除，无法恢复。之后的对话会重新开始积累。`
              : pending.content
          }
          confirmLabel={pending === "all" ? "全部清空" : "删除"}
          busy={working}
          onConfirm={handleConfirmed}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
