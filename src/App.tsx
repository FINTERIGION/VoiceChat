import { useCallback, useRef, useState } from "react";
import { MessageCircle, Settings as SettingsIcon, Users } from "lucide-react";
import ConfirmDialog from "./components/ConfirmDialog";
import UpdateBanner from "./components/UpdateBanner";
import { useT } from "./lib/i18n";
import { tabBtn } from "./lib/ui";
import CharacterEdit from "./routes/CharacterEdit";
import CharacterList from "./routes/CharacterList";
import Chat from "./routes/Chat";
import MemoryManager from "./routes/MemoryManager";
import Settings, { type SettingsSection } from "./routes/Settings";

type Tab = "chat" | "characters" | "settings";

const TABS = [
  { id: "chat", label: "nav.chat", icon: MessageCircle },
  { id: "characters", label: "nav.characters", icon: Users },
  { id: "settings", label: "nav.settings", icon: SettingsIcon },
] as const;

function App() {
  const t = useT();
  const [tab, setTab] = useState<Tab>("chat");
  const [editingId, setEditingId] = useState<string | "new" | null>(null);
  // The character shown in the Characters tab's detail pane. Held here so
  // the editor and the memory page, which replace the tab while open, hand
  // back to the same one; `null` lets the tab pick the current character.
  const [selectedCharacterId, setSelectedCharacterId] = useState<
    string | null
  >(null);
  const [viewingMemory, setViewingMemory] = useState<{
    id: string;
    name: string;
  } | null>(null);
  // Whether the character editor holds unsaved edits. Any tab click leaves
  // it (the Characters tab itself included, which goes back to the list), so
  // that is where the question gets asked.
  const editDirty = useRef(false);
  const [pendingTab, setPendingTab] = useState<Tab | null>(null);
  // Held here rather than in Settings, which unmounts with its tab, so
  // coming back finds the section that was open — and so the Chat tab's
  // first-run prompt can open Settings straight at the API key.
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("general");
  const handleDirtyChange = useCallback((dirty: boolean) => {
    editDirty.current = dirty;
  }, []);

  function closeEditor() {
    editDirty.current = false;
    setEditingId(null);
  }

  function goToTab(next: Tab) {
    setTab(next);
    closeEditor();
    setViewingMemory(null);
  }

  function openSettingsAt(section: SettingsSection) {
    setSettingsSection(section);
    handleTabClick("settings");
  }

  function handleTabClick(next: Tab) {
    if (editingId !== null && editDirty.current) {
      setPendingTab(next);
      return;
    }
    goToTab(next);
  }

  return (
    // `scheme-dark` so the controls the webview draws itself — a select's
    // drop-down list, the audio player — come out dark too. Set here rather
    // than on `:root`, where it would also darken the subtitle window's
    // canvas, which has to stay transparent.
    <div className="flex h-screen flex-col bg-neutral-950 scheme-dark">
      <nav className="flex gap-1 border-b border-neutral-800 px-4 pt-3">
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => handleTabClick(id)}
            aria-current={tab === id ? "page" : undefined}
            className={`${tabBtn(tab === id)} gap-1.5`}
          >
            <Icon className="size-4" />
            {t(label)}
          </button>
        ))}
      </nav>
      <UpdateBanner
        hidden={tab === "settings" && settingsSection === "about"}
        onView={() => openSettingsAt("about")}
      />
      <div className="flex-1 overflow-y-auto">
        {/* Kept mounted (hidden via CSS) instead of conditionally rendered so
            switching tabs doesn't wipe the in-memory chat transcript. Chat
            replays it from the database on mount, but only as far back as the
            conversation being recorded right now — anything said before that,
            or with recording switched off, exists nowhere else. */}
        <div className={tab === "chat" ? "h-full" : "hidden"}>
          <Chat
            active={tab === "chat"}
            onOpenSettings={() => openSettingsAt("connection")}
            onOpenCharacters={() => {
              setSelectedCharacterId(null);
              handleTabClick("characters");
            }}
          />
        </div>
        {tab === "characters" &&
          (viewingMemory !== null ? (
            <MemoryManager
              characterId={viewingMemory.id}
              characterName={viewingMemory.name}
              onBack={() => setViewingMemory(null)}
            />
          ) : editingId !== null ? (
            <CharacterEdit
              id={editingId}
              onDone={(savedId) => {
                if (savedId) setSelectedCharacterId(savedId);
                closeEditor();
              }}
              onDirtyChange={handleDirtyChange}
            />
          ) : (
            <CharacterList
              selectedId={selectedCharacterId}
              onSelect={setSelectedCharacterId}
              onEdit={setEditingId}
              onViewMemory={(id, name) => setViewingMemory({ id, name })}
              onOpenChat={() => goToTab("chat")}
            />
          ))}
        {tab === "settings" && (
          <Settings
            section={settingsSection}
            onSectionChange={setSettingsSection}
          />
        )}
      </div>

      {pendingTab !== null && (
        <ConfirmDialog
          title={t("characterEdit.discardTitle")}
          body={t("characterEdit.discardBody")}
          confirmLabel={t("characterEdit.discardConfirm")}
          onConfirm={() => {
            goToTab(pendingTab);
            setPendingTab(null);
          }}
          onCancel={() => setPendingTab(null)}
        />
      )}
    </div>
  );
}

export default App;
