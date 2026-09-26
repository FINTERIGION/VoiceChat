import { useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Brain,
  Camera,
  Import,
  MessageCircle,
  Pencil,
  Plus,
  Share2,
  Trash2,
} from "lucide-react";
import Avatar from "../components/Avatar";
import AvatarDialog from "../components/AvatarDialog";
import ConfirmDialog from "../components/ConfirmDialog";
import ImportCharacterDialog from "../components/ImportCharacterDialog";
import ShareCharacterDialog from "../components/ShareCharacterDialog";
import { formatRelative } from "../lib/format";
import { useI18n } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { describeVoice, LANGUAGE_LABEL } from "../lib/labels";
import type { Character, SharedCharacter } from "../lib/types";
import { btn, interactive } from "../lib/ui";

/** What the detail pane says about a character beyond its own fields. */
interface Stats {
  memories: number;
  conversations: number;
  /** When the most recent conversation started; `null` if there is none. */
  lastChat: string | null;
}

/**
 * The Characters tab: every character down the side, the selected one in
 * full on the right.
 *
 * Looking and switching are separate on purpose. Selecting a row only shows
 * that character; making them the one you talk to is the detail pane's
 * "Start chatting" (or Enter on the row), which switches and goes to the
 * Chat tab in one step — switching ends the conversation in progress, so it
 * shouldn't happen on the way to reading someone's persona.
 *
 * `selectedId` lives in App, so coming back from the editor or the memory
 * page lands on the character that was open.
 */
export default function CharacterList({
  selectedId,
  onSelect,
  onEdit,
  onViewMemory,
  onOpenChat,
}: {
  selectedId: string | null;
  onSelect: (id: string) => void;
  onEdit: (id: string | "new") => void;
  onViewMemory: (id: string, name: string) => void;
  /** Called once the character is current, to show the Chat tab. */
  onOpenChat: () => void;
}) {
  const { t, lang } = useI18n();
  const list = useRef<HTMLUListElement>(null);
  const [characters, setCharacters] = useState<Character[]>([]);
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [switching, setSwitching] = useState(false);
  const [stats, setStats] = useState<Stats | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Character | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [avatarFor, setAvatarFor] = useState<Character | null>(null);
  const [sharing, setSharing] = useState<Character | null>(null);
  // A file picked and checked, waiting for the user to confirm the import.
  const [importing, setImporting] = useState<SharedCharacter | null>(null);
  const [opening, setOpening] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [all, current] = await Promise.all([
        ipc.listCharacters(),
        ipc.getCurrentCharacterId(),
      ]);
      setCharacters(all);
      setCurrentId(current);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  const selected = characters.find((c) => c.id === selectedId) ?? null;

  // Nothing picked yet (or the pick was just deleted): show the character
  // you are talking to, which is what someone opening this tab most likely
  // wants to look at.
  useEffect(() => {
    if (loading || characters.length === 0 || selected) return;
    onSelect(
      characters.some((c) => c.id === currentId)
        ? currentId!
        : characters[0].id,
    );
  }, [loading, characters, currentId, selected, onSelect]);

  useEffect(() => {
    if (!selected) return;
    let live = true;
    setStats(null);
    Promise.all([
      ipc.listMemories(selected.id),
      ipc.listConversations(selected.id),
    ])
      .then(([memories, conversations]) => {
        if (!live) return;
        // Every connect opens a conversation, so one nobody spoke in is not
        // a conversation anyone had.
        const had = conversations.filter((c) => c.message_count > 0);
        setStats({
          memories: memories.length,
          conversations: had.length,
          lastChat: had.reduce<string | null>(
            (latest, c) =>
              latest === null || c.started_at > latest ? c.started_at : latest,
            null,
          ),
        });
      })
      .catch(() => {
        // The pane still has everything else to show.
      });
    return () => {
      live = false;
    };
  }, [selected?.id]);

  async function handleChat(c: Character) {
    if (switching) return;
    if (c.id !== currentId) {
      setSwitching(true);
      setError(null);
      try {
        await ipc.switchCharacter(c.id);
        setCurrentId(c.id);
      } catch (e) {
        setError(String(e));
        return;
      } finally {
        setSwitching(false);
      }
    }
    onOpenChat();
  }

  async function handleDelete() {
    if (!pendingDelete || characters.length <= 1) {
      return;
    }
    setDeleting(true);
    setError(null);
    try {
      await ipc.deleteCharacter(pendingDelete.id);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
      // Dismissed either way: a failure is reported on the page, where it
      // stays readable after the dialog is gone.
      setPendingDelete(null);
    }
  }

  // "Exported to …" is about the character it was said beside.
  useEffect(() => {
    setNotice(null);
  }, [selectedId]);

  async function handleOpenFile() {
    setOpening(true);
    setError(null);
    setNotice(null);
    try {
      // `null` is the picker dismissed.
      const shared = await ipc.openCharacterFile();
      if (shared) setImporting(shared);
    } catch (e) {
      setError(String(e));
    } finally {
      setOpening(false);
    }
  }

  async function handleImported(created: Character) {
    setImporting(null);
    await refresh();
    // Shown, not switched to: importing shouldn't end the conversation in
    // progress.
    onSelect(created.id);
  }

  async function applyAvatar(c: Character, avatarPath: string) {
    const updated = await ipc.setCharacterAvatar(c.id, avatarPath);
    setCharacters((all) => all.map((x) => (x.id === updated.id ? updated : x)));
    setAvatarFor(null);
  }

  /** Up/Down walk the list; Enter on a row starts chatting with it. */
  function handleListKeyDown(e: React.KeyboardEvent) {
    const at = characters.findIndex((c) => c.id === selectedId);
    let next: number | null = null;
    if (e.key === "ArrowDown") next = Math.min(characters.length - 1, at + 1);
    else if (e.key === "ArrowUp") next = Math.max(0, at - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = characters.length - 1;
    if (next === null || next === at) return;
    e.preventDefault();
    const id = characters[next].id;
    onSelect(id);
    list.current
      ?.querySelector<HTMLButtonElement>(`[data-id="${CSS.escape(id)}"]`)
      ?.focus();
  }

  if (loading) {
    return <p className="p-6 text-sm text-neutral-500">{t("common.loading")}</p>;
  }

  return (
    <div className="flex h-full text-neutral-100">
      {/* Wide enough for a name at `character::NAME_MAX_WIDTH` beside its avatar. */}
      <aside className="flex w-64 shrink-0 flex-col border-r border-neutral-800 bg-neutral-950">
        <div className="flex gap-2 px-3 pt-4">
          <button
            onClick={() => onEdit("new")}
            className={`${btn.outline} min-w-0 flex-1 gap-1.5 py-2 text-sm`}
          >
            <Plus className="size-4 shrink-0" />
            <span className="truncate">{t("characters.new")}</span>
          </button>
          <button
            onClick={handleOpenFile}
            disabled={opening}
            title={t("characters.importTitle")}
            className={`${btn.outline} shrink-0 gap-1.5 px-3 py-2 text-sm`}
          >
            <Import className="size-4" />
            {t("characters.import")}
          </button>
        </div>
        <h2 className="px-4 pt-3.5 pb-2 text-xs font-medium tracking-wide text-neutral-500 uppercase">
          {t("characters.title")}
        </h2>
        <ul
          ref={list}
          aria-label={t("characters.title")}
          onKeyDown={handleListKeyDown}
          className="flex-1 space-y-0.5 overflow-y-auto px-2 pb-4"
        >
          {characters.map((c) => {
            const isSelected = c.id === selectedId;
            const isCurrent = c.id === currentId;
            return (
              <li key={c.id}>
                <button
                  data-id={c.id}
                  onClick={() => onSelect(c.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void handleChat(c);
                    }
                  }}
                  aria-current={isSelected ? "true" : undefined}
                  // Only the selected row is a Tab stop; the arrow keys
                  // move between the rest.
                  tabIndex={isSelected ? 0 : -1}
                  className={`${interactive} flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left ${
                    isSelected
                      ? "bg-neutral-800 text-neutral-100"
                      : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
                  }`}
                >
                  <Avatar
                    id={c.id}
                    name={c.name}
                    avatarPath={c.avatar_path}
                    size="md"
                    current={isCurrent}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm" title={c.name}>
                      {c.name}
                    </span>
                    <span
                      className={`block truncate text-xs ${
                        isCurrent ? "text-emerald-400/90" : "text-neutral-500"
                      }`}
                    >
                      {isCurrent
                        ? t("characters.current")
                        : t(LANGUAGE_LABEL[c.language])}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </aside>

      <section className="min-w-0 flex-1 overflow-y-auto">
        {error && (
          <p
            role="alert"
            className="mx-6 mt-6 rounded-lg bg-red-500/10 px-3 py-2 text-sm text-red-400"
          >
            {error}
          </p>
        )}
        {notice && (
          <p
            role="status"
            className="mx-6 mt-6 rounded-lg bg-emerald-500/10 px-3 py-2 text-sm break-all text-emerald-400"
          >
            {notice}
          </p>
        )}
        {selected && (
          <Detail
            character={selected}
            isCurrent={selected.id === currentId}
            stats={stats}
            locale={lang}
            switching={switching}
            canDelete={characters.length > 1}
            onChat={() => handleChat(selected)}
            onEdit={() => onEdit(selected.id)}
            onViewMemory={() => onViewMemory(selected.id, selected.name)}
            onChangeAvatar={() => setAvatarFor(selected)}
            onShare={() => {
              setNotice(null);
              setSharing(selected);
            }}
            onDelete={() => setPendingDelete(selected)}
          />
        )}
      </section>

      {sharing && (
        <ShareCharacterDialog
          character={sharing}
          onExported={(path) => {
            setSharing(null);
            setNotice(t("characters.shared", { path }));
          }}
          onClose={() => setSharing(null)}
        />
      )}

      {importing && (
        <ImportCharacterDialog
          shared={importing}
          onImported={handleImported}
          onClose={() => setImporting(null)}
        />
      )}

      {avatarFor && (
        <AvatarDialog
          characterName={avatarFor.name}
          persona={avatarFor.persona}
          onSaved={(path) => applyAvatar(avatarFor, path)}
          onClose={() => setAvatarFor(null)}
        />
      )}

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

function Detail({
  character: c,
  isCurrent,
  stats,
  locale,
  switching,
  canDelete,
  onChat,
  onEdit,
  onViewMemory,
  onChangeAvatar,
  onShare,
  onDelete,
}: {
  character: Character;
  isCurrent: boolean;
  stats: Stats | null;
  locale: string;
  switching: boolean;
  canDelete: boolean;
  onChat: () => void;
  onEdit: () => void;
  onViewMemory: () => void;
  onChangeAvatar: () => void;
  onShare: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const avatarLabel = t(c.avatar_path ? "avatar.change" : "avatar.set");

  return (
    <div className="mx-auto max-w-2xl space-y-6 p-6">
      <header className="flex items-start gap-5">
        <button
          onClick={onChangeAvatar}
          title={avatarLabel}
          aria-label={avatarLabel}
          className={`${interactive} group relative shrink-0 rounded-full`}
        >
          <Avatar
            id={c.id}
            name={c.name}
            avatarPath={c.avatar_path}
            size="xl"
          />
          <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/55 text-neutral-100 opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100">
            <Camera className="size-5" />
          </span>
        </button>

        <div className="min-w-0 flex-1 pt-1">
          <div className="flex min-w-0 items-center gap-2">
            <h1 className="min-w-0 text-xl font-semibold break-words">
              {c.name}
            </h1>
            {isCurrent && (
              <span
                title={t("characters.currentTitle")}
                className="inline-flex shrink-0 items-center gap-1.5 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-400"
              >
                <span className="size-1.5 rounded-full bg-emerald-400" />
                {t("characters.current")}
              </span>
            )}
          </div>
          <p className="mt-1 truncate text-sm text-neutral-400">
            {t(LANGUAGE_LABEL[c.language])} ·{" "}
            {describeVoice(t, c.voice_kind, c.voice_id)}
          </p>

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <button
              onClick={onChat}
              disabled={switching}
              title={isCurrent ? undefined : t("characters.startChatTitle")}
              className={`${btn.primary} gap-1.5 px-3.5 py-1.5 text-sm font-medium`}
            >
              {isCurrent ? (
                <ArrowRight className="size-4" />
              ) : (
                <MessageCircle className="size-4" />
              )}
              {switching
                ? t("characters.switching")
                : t(isCurrent ? "characters.backToChat" : "characters.startChat")}
            </button>
            <button
              onClick={onEdit}
              className={`${btn.outline} gap-1.5 px-3 py-1.5 text-sm`}
            >
              <Pencil className="size-3.5" />
              {t("common.edit")}
            </button>
            <button
              onClick={onViewMemory}
              className={`${btn.outline} gap-1.5 px-3 py-1.5 text-sm`}
            >
              <Brain className="size-3.5" />
              {t("characters.memory")}
            </button>
            <button
              onClick={onShare}
              title={t("characters.shareTitle")}
              className={`${btn.outline} gap-1.5 px-3 py-1.5 text-sm`}
            >
              <Share2 className="size-3.5" />
              {t("characters.share")}
            </button>
            {/* Set apart from the everyday actions, and quieter than them. */}
            <button
              onClick={onDelete}
              disabled={!canDelete}
              title={canDelete ? undefined : t("characters.deleteLastTitle")}
              className={`${btn.dangerGhost} ml-auto gap-1.5 px-2.5 py-1.5 text-sm`}
            >
              <Trash2 className="size-3.5" />
              {t("common.delete")}
            </button>
          </div>
        </div>
      </header>

      <dl className="grid grid-cols-3 gap-3">
        <Stat label={t("characters.stats.memory")}>
          {!c.memory_enabled
            ? t("characters.stats.memoryOff")
            : stats
              ? t("characters.stats.memoryOn", { count: stats.memories })
              : t("characters.stats.memoryOnPlain")}
        </Stat>
        <Stat label={t("characters.stats.history")}>
          {t("characters.stats.historyValue", { count: c.max_history_turns })}
        </Stat>
        <Stat
          label={t("characters.stats.conversations")}
          sub={
            stats?.lastChat
              ? t("characters.stats.lastChat", {
                  when: formatRelative(stats.lastChat, locale),
                })
              : undefined
          }
        >
          {stats
            ? t("characters.stats.conversationsValue", {
                count: stats.conversations,
              })
            : "—"}
        </Stat>
      </dl>

      <Prose title={t("characterEdit.persona")}>
        {c.persona || t("characters.noPersona")}
      </Prose>
      <Prose title={t("characterEdit.speechHabits")}>
        {c.speech_habits || t("characters.noSpeechHabits")}
      </Prose>
    </div>
  );
}

function Stat({
  label,
  sub,
  children,
}: {
  label: string;
  sub?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-neutral-800 bg-neutral-900/40 px-4 py-3">
      <dt className="text-xs text-neutral-500">{label}</dt>
      <dd className="mt-1 truncate text-sm text-neutral-100">{children}</dd>
      {sub && <dd className="mt-0.5 truncate text-xs text-neutral-500">{sub}</dd>}
    </div>
  );
}

function Prose({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="mb-1.5 text-xs font-medium text-neutral-500">{title}</h2>
      <p className="text-sm leading-relaxed whitespace-pre-wrap break-words text-neutral-300">
        {children}
      </p>
    </section>
  );
}
