import { useState } from "react";
import { useT } from "./lib/i18n";
import { tabBtn } from "./lib/ui";
import CharacterEdit from "./routes/CharacterEdit";
import CharacterList from "./routes/CharacterList";
import Chat from "./routes/Chat";
import MemoryManager from "./routes/MemoryManager";
import Settings from "./routes/Settings";

type Tab = "chat" | "characters" | "settings";

const TAB_LABEL = {
  chat: "nav.chat",
  characters: "nav.characters",
  settings: "nav.settings",
} as const;

function App() {
  const t = useT();
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
        {(["chat", "characters", "settings"] as const).map((tab_) => (
          <button
            key={tab_}
            onClick={() => {
              setTab(tab_);
              resetCharactersView();
            }}
            className={tabBtn(tab === tab_)}
          >
            {t(TAB_LABEL[tab_])}
          </button>
        ))}
      </nav>
      <div className="flex-1 overflow-y-auto">
        {/* Kept mounted (hidden via CSS) instead of conditionally rendered so
            switching tabs doesn't wipe the in-memory chat transcript. Chat
            replays it from the database on mount, but only as far back as the
            conversation being recorded right now — anything said before that,
            or with recording switched off, exists nowhere else. */}
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
