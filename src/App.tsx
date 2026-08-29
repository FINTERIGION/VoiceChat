import { useState } from "react";
import CharacterEdit from "./routes/CharacterEdit";
import CharacterList from "./routes/CharacterList";
import Chat from "./routes/Chat";
import MemoryManager from "./routes/MemoryManager";
import Settings from "./routes/Settings";

type Tab = "chat" | "characters" | "settings";

function App() {
  const [tab, setTab] = useState<Tab>("chat");
  const [editingId, setEditingId] = useState<string | "new" | null>(null);
  const [viewingMemory, setViewingMemory] = useState<{
    id: string;
    name: string;
  } | null>(null);

  function resetCharactersView() {
    setEditingId(null);
    setViewingMemory(null);
  }

  return (
    <div className="flex h-screen flex-col bg-neutral-950">
      <nav className="flex gap-1 border-b border-neutral-800 px-4 pt-3">
        {(["chat", "characters", "settings"] as const).map((t) => (
          <button
            key={t}
            onClick={() => {
              setTab(t);
              resetCharactersView();
            }}
            className={`rounded-t-lg px-3 py-1.5 text-sm ${
              tab === t
                ? "bg-neutral-900 text-neutral-100"
                : "text-neutral-500 hover:text-neutral-300"
            }`}
          >
            {t === "chat" ? "对话" : t === "characters" ? "角色" : "设置"}
          </button>
        ))}
      </nav>
      <div className="flex-1 overflow-y-auto">
        {/* Kept mounted (hidden via CSS) instead of conditionally rendered so
            switching tabs doesn't wipe the in-memory chat transcript, which
            has no backend history to reload from. */}
        <div className={tab === "chat" ? "h-full" : "hidden"}>
          <Chat />
        </div>
        {tab === "characters" &&
          (viewingMemory !== null ? (
            <MemoryManager
              characterId={viewingMemory.id}
              characterName={viewingMemory.name}
              onBack={() => setViewingMemory(null)}
            />
          ) : editingId !== null ? (
            <CharacterEdit id={editingId} onDone={() => setEditingId(null)} />
          ) : (
            <CharacterList
              onEdit={setEditingId}
              onViewMemory={(id, name) => setViewingMemory({ id, name })}
            />
          ))}
        {tab === "settings" && <Settings />}
      </div>
    </div>
  );
}

export default App;
