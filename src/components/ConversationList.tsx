import { useEffect, useRef, useState } from "react";
import {
  PanelLeftClose,
  PanelLeftOpen,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";
import ConfirmDialog from "./ConfirmDialog";
import { useI18n } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btn, fieldCompact, interactive } from "../lib/ui";
import type { ConversationSummary } from "../lib/types";

/**
 * Renders `started_at` (an RFC 3339 timestamp) as something short enough for
 * a sidebar row: the time alone for today, the date otherwise. An
 * unparseable timestamp falls back to the raw string, which is at least
 * still readable.
 */
function formatStartedAt(iso: string, locale: string): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return iso;
  const now = new Date();
  const sameDay =
    at.getFullYear() === now.getFullYear() &&
    at.getMonth() === now.getMonth() &&
    at.getDate() === now.getDate();
  return sameDay
    ? at.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : at.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

/**
 * Whether the sidebar was left collapsed. The Chat tab unmounts whenever
 * another tab is opened, so component state alone would pop it back open on
 * every return. It's a layout nicety rather than a setting, hence the
 * webview's own storage instead of the backend's; when that storage is
 * unavailable the sidebar simply starts out expanded.
 */
const COLLAPSED_KEY = "history.collapsed";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
  } catch {
    // Costs nothing but remembering it next time.
  }
}

/**
 * The Chat tab's history sidebar.
 *
 * `selectedId` is the conversation being reviewed, and `null` while the live
 * transcript is on screen. `activeId` is the one the session is writing to
 * right now — usually the newest row, and the one whose row takes you back
 * to the live view rather than into a replay.
 */
export default function ConversationList({
  conversations,
  activeId,
  selectedId,
  onSelect,
  onChanged,
  onNewConversation,
}: {
  conversations: ConversationSummary[];
  activeId: string | null;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  /** A rename or delete landed; the owner refetches and reconciles. */
  onChanged: () => void;
  /** "开启新对话": saves the conversation currently live and starts a fresh one. */
  onNewConversation: () => void;
}) {
  const { t, lang } = useI18n();
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [pending, setPending] = useState<ConversationSummary | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const renameInput = useRef<HTMLInputElement>(null);

  // Nothing to save yet if the live conversation hasn't taken a single
  // message (or nothing is connected at all) — starting "new" from there
  // would just be a pointless reconnect.
  const active = conversations.find((c) => c.id === activeId);
  const canStartNew = !!active && active.message_count > 0;

  useEffect(() => {
    renameInput.current?.select();
  }, [renamingId]);

  function startRename(c: ConversationSummary) {
    setError(null);
    setRenamingId(c.id);
    setDraft(c.title ?? "");
  }

  async function commitRename(id: string) {
    const title = draft.trim();
    const previous = conversations.find((c) => c.id === id)?.title ?? "";
    setRenamingId(null);
    // An emptied or unchanged name reads as backing out, not as a rename.
    // The backend rejects an empty one anyway, and surfacing that as an
    // error would put a red banner under what the user meant as a cancel.
    if (title === "" || title === previous) return;
    try {
      await ipc.renameConversation(id, title);
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  function toggleCollapsed() {
    const next = !collapsed;
    setCollapsed(next);
    writeCollapsed(next);
  }

  async function confirmDelete() {
    if (!pending) return;
    setDeleting(true);
    try {
      await ipc.deleteConversation(pending.id);
      // Nothing left to review — fall back to the live transcript.
      if (selectedId === pending.id) onSelect(null);
      setPending(null);
      setError(null);
      onChanged();
    } catch (e) {
      // The one expected failure is deleting the conversation still being
      // spoken into, which the backend refuses with an explanation.
      setError(String(e));
      setPending(null);
    } finally {
      setDeleting(false);
    }
  }

  const toggleLabel = t(collapsed ? "history.expand" : "history.collapse");

  return (
    // Only the width animates. The expanded contents keep their full width
    // throughout and are clipped instead, so opening the sidebar reveals the
    // list rather than reflowing it through every width on the way.
    <aside
      className={`flex shrink-0 overflow-hidden border-r border-neutral-800 bg-neutral-950 transition-[width] duration-200 ${
        collapsed ? "w-12" : "w-60"
      }`}
    >
      {collapsed ? (
        // The rail keeps "new conversation" within reach, in the same spot
        // as the full button, with the toggle below where the heading was.
        <div className="flex w-12 shrink-0 flex-col items-center gap-2 pt-4">
          <button
            onClick={onNewConversation}
            disabled={!canStartNew}
            title={t(canStartNew ? "history.new" : "history.newDisabledTitle")}
            aria-label={t("history.new")}
            className={`${btn.outline} h-9 w-9`}
          >
            <Plus className="size-4" />
          </button>
          <button
            onClick={toggleCollapsed}
            title={toggleLabel}
            aria-label={toggleLabel}
            aria-expanded={false}
            className={`${btn.ghost} h-9 w-9`}
          >
            <PanelLeftOpen className="size-4" />
          </button>
        </div>
      ) : (
        <div className="flex w-60 shrink-0 flex-col">
          <div className="px-3 pt-4">
            <button
              onClick={onNewConversation}
              disabled={!canStartNew}
              title={canStartNew ? undefined : t("history.newDisabledTitle")}
              className={`${btn.outline} w-full gap-1.5 py-2 text-sm`}
            >
              <Plus className="size-4" />
              {t("history.new")}
            </button>
          </div>

          <div className="flex items-center justify-between pb-1.5 pl-4 pr-2 pt-2.5">
            <h2 className="text-xs font-medium uppercase tracking-wide text-neutral-500">
              {t("history.title")}
            </h2>
            <button
              onClick={toggleCollapsed}
              title={toggleLabel}
              aria-label={toggleLabel}
              aria-expanded={true}
              className={`${btn.ghost} h-7 w-7`}
            >
              <PanelLeftClose className="size-4" />
            </button>
          </div>

          {error && (
            <p
              role="alert"
              className="mx-3 mb-2 rounded-lg bg-red-500/10 px-3 py-2 text-xs leading-relaxed text-red-400"
            >
              {error}
            </p>
          )}

          <div className="flex-1 overflow-y-auto px-2 pb-4">
            {conversations.length === 0 ? (
              <p className="px-2 text-xs leading-relaxed text-neutral-500">
                {t("history.empty")}
              </p>
            ) : (
              <ul className="space-y-0.5">
                {conversations.map((c) => {
                  const isLive = c.id === activeId;
                  const isCurrent = isLive ? selectedId === null : selectedId === c.id;
                  const isRenaming = renamingId === c.id;
                  // While renaming, the row stops being a button: it holds a
                  // text field, so it must not swallow clicks meant for the
                  // caret, take focus of its own, or claim `role="button"` over
                  // interactive content.
                  const rowProps: React.HTMLAttributes<HTMLDivElement> = isRenaming
                    ? {}
                    : {
                        role: "button",
                        tabIndex: 0,
                        // Picking the conversation the session is writing to
                        // means "show me what is going on now", which is the
                        // live transcript — not a replay of the part of it that
                        // has been stored so far.
                        onClick: () => onSelect(isLive ? null : c.id),
                        onKeyDown: (e) => {
                          // Enter or Space on one of the buttons inside the row
                          // belongs to that button.
                          if (e.target !== e.currentTarget) return;
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            onSelect(isLive ? null : c.id);
                          }
                        },
                      };
                  return (
                    <li key={c.id}>
                      <div
                        {...rowProps}
                        className={`group flex w-full items-center gap-1 rounded-lg px-2.5 py-2 text-left ${
                          isRenaming
                            ? "bg-neutral-800 text-neutral-100"
                            : `${interactive} ${
                                isCurrent
                                  ? "bg-neutral-800 text-neutral-100"
                                  : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
                              }`
                        }`}
                      >
                        <div className="min-w-0 flex-1">
                          {/* The field takes the name's place, leaving the row
                              and the line below it where they were — renaming
                              shouldn't make the conversation you are editing
                              disappear from the list. */}
                          {isRenaming ? (
                            <input
                              ref={renameInput}
                              value={draft}
                              aria-label={t("history.renameLabel")}
                              onChange={(e) => setDraft(e.target.value)}
                              onBlur={() => commitRename(c.id)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") commitRename(c.id);
                                if (e.key === "Escape") setRenamingId(null);
                              }}
                              className={`${fieldCompact} -my-px w-full`}
                            />
                          ) : (
                            <p className="truncate text-sm">
                              {c.title || c.preview || t("history.untitled")}
                            </p>
                          )}
                          <p className="mt-0.5 truncate text-xs text-neutral-500">
                            {isLive
                              ? t("history.live")
                              : formatStartedAt(c.started_at, lang)}
                            {" · "}
                            {t("history.messageCount", { count: c.message_count })}
                          </p>
                        </div>
                        {/* Idle rows stay uncluttered; the actions fade in with
                            the cursor, and with keyboard focus so they stay
                            reachable without a mouse. They go while renaming, so
                            the field gets the row's full width and nothing sits
                            next to it waiting to be mis-clicked. */}
                        {!isRenaming && (
                          <div className="flex shrink-0 gap-0.5 opacity-0 transition-opacity duration-200 group-focus-within:opacity-100 group-hover:opacity-100">
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                startRename(c);
                              }}
                              title={t("history.renameTitle")}
                              aria-label={t("history.renameTitle")}
                              className={`${btn.ghost} p-1.5`}
                            >
                              <Pencil className="size-3.5" />
                            </button>
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                setPending(c);
                              }}
                              title={t("common.delete")}
                              aria-label={t("common.delete")}
                              className={`${btn.dangerGhost} p-1.5`}
                            >
                              <Trash2 className="size-3.5" />
                            </button>
                          </div>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      )}

      {pending && (
        <ConfirmDialog
          title={t("history.deleteTitle")}
          // "What it remembers stays" is only true of a conversation that
          // went into memory in the first place.
          body={t(
            pending.memorized
              ? "history.deleteBody"
              : "history.deleteBodyUnmemorized",
          )}
          busy={deleting}
          onConfirm={confirmDelete}
          onCancel={() => setPending(null)}
        />
      )}
    </aside>
  );
}
