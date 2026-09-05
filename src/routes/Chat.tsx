import { useCallback, useEffect, useRef, useState } from "react";
import ConversationList from "../components/ConversationList";
import { useT, type MessageKey } from "../lib/i18n";
import {
  ipc,
  onChatCharacter,
  onChatConversations,
  onChatLevel,
  onChatMic,
  onChatRecording,
  onChatState,
  onChatTranscript,
  subscribe,
} from "../lib/ipc";
import type {
  ChatMessage,
  ChatStateEvent,
  ConversationSummary,
} from "../lib/types";
import { btn, btnBase, hoverGlow } from "../lib/ui";

interface Bubble {
  role: "user" | "assistant";
  text: string;
  key: number;
}

const STATE_LABEL: Record<ChatStateEvent["state"], MessageKey> = {
  idle: "chat.state.idle",
  connecting: "chat.state.connecting",
  listening: "chat.state.listening",
  thinking: "chat.state.thinking",
  speaking: "chat.state.speaking",
  error: "chat.state.error",
};

const STATE_COLOR: Record<ChatStateEvent["state"], string> = {
  idle: "bg-neutral-500",
  connecting: "bg-amber-400 animate-pulse",
  listening: "bg-emerald-400 animate-pulse",
  thinking: "bg-sky-400 animate-pulse",
  speaking: "bg-violet-400 animate-pulse",
  error: "bg-red-500",
};

export default function Chat() {
  const t = useT();
  const [state, setState] = useState<ChatStateEvent>({ state: "idle" });
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [level, setLevel] = useState(0);
  const [micOpen, setMicOpen] = useState(false);
  // `null` until the first `chat:recording` event, and again whenever the
  // current character has memory switched off — in both cases there is no
  // choice to offer, so the toggle is not rendered at all.
  const [recording, setRecording] = useState<boolean | null>(null);
  const [hotkey, setHotkey] = useState<string | null>(null);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  // The conversation the live session is writing to, `null` when it isn't
  // recording one (the character has memory off, or nothing is connected).
  const [activeId, setActiveId] = useState<string | null>(null);
  // The past conversation being reviewed, `null` while the live transcript
  // is on screen. Its messages are held separately from `bubbles` so a turn
  // arriving mid-review can't write into what is being read.
  const [reviewing, setReviewing] = useState<ConversationSummary | null>(null);
  const [reviewBubbles, setReviewBubbles] = useState<Bubble[]>([]);
  const [reviewError, setReviewError] = useState<string | null>(null);
  const nextKey = useRef(0);
  const assistantKey = useRef<number | null>(null);
  // Keys of user bubbles reserved (via the empty, `done: false` placeholder
  // emitted at speech_stopped) but not yet filled in by the async
  // transcription result. FIFO since transcriptions resolve in the order
  // their turns were committed.
  const pendingUserKeys = useRef<number[]>([]);
  // The history list is per character, and `chat:conversations` says only
  // that the list changed — not whose. The subscriptions are registered once
  // and so close over the character id at mount; this ref is what lets them
  // refetch for whoever is current now.
  const characterIdRef = useRef<string | null>(null);

  const refreshConversations = useCallback((forCharacter: string | null) => {
    characterIdRef.current = forCharacter;
    if (!forCharacter) {
      setConversations([]);
      return;
    }
    ipc
      .listConversations(forCharacter)
      .then(setConversations)
      .catch(() => setConversations([]));
  }, []);

  useEffect(() => {
    // Covers remounting this tab (switching tabs unmounts it, see App.tsx)
    // while the mic was already open via the global hotkey — after this,
    // `chat:mic` keeps it in sync with hotkey presses made from anywhere.
    ipc.getMicOpen().then(setMicOpen);
    ipc.getHotkey().then(setHotkey);
    // Set once whatever the replay below is fetching stops being what
    // belongs on screen — this tab unmounting, or the live session moving to
    // another character.
    let restoreStale = false;

    // Likewise for a session that opened its conversation before this tab
    // was listening for `chat:conversations` — and, since `bubbles` lives
    // only in this component, replay what that conversation already holds.
    // Without this a reload of the webview (Vite's HMR client does one every
    // time the window comes back from the background under `tauri dev`; a
    // renderer restart or an app restart does one in a shipped build) leaves
    // the conversation still being recorded looking empty, with no way to
    // reach it: its history row is the live one, and selecting that comes
    // back here.
    ipc.getActiveConversationId().then(async (id) => {
      setActiveId(id);
      if (id === null) return;
      let messages: ChatMessage[];
      try {
        messages = await ipc.getConversationMessages(id);
      } catch {
        // Nothing to fall back to, and the live session is unaffected, so
        // leaving the transcript empty is no worse than not having tried.
        return;
      }
      if (restoreStale || messages.length === 0) return;
      const restored = messages.map((m) => ({
        role: m.role,
        text: m.text,
        key: nextKey.current++,
      }));
      // Turns that landed while this was in flight keep their place at the
      // end; the replay belongs in front of them.
      setBubbles((prev) => [...restored, ...prev]);
    });
    ipc.getCurrentCharacterId().then(refreshConversations);

    // Drops user bubbles still waiting on a transcription that can no longer
    // arrive, and empties the pending queue with them. Without this, a turn
    // the backend never transcribed (the socket died after its
    // `speech_stopped` placeholder was emitted) leaves a stale key at the
    // head of the FIFO, and the *next* turn's transcript gets written into
    // that older bubble instead — putting the user's words above the reply
    // that prompted them, and leaving the newest bubble blank forever.
    function discardPendingUserBubbles() {
      if (pendingUserKeys.current.length === 0) return;
      const orphaned = new Set(pendingUserKeys.current);
      pendingUserKeys.current = [];
      setBubbles((prev) => prev.filter((b) => !orphaned.has(b.key)));
    }

    const cleanups = [
      subscribe(() =>
        onChatCharacter((id) => {
          // A different character is now live — its conversation has no
          // relation to whatever was on screen, and the new session hasn't
          // opened one to replay in its place, so just clear the transcript.
          // The history list is per character, so it and any open review go
          // too, and a replay still in flight belongs to the character being
          // left behind.
          restoreStale = true;
          pendingUserKeys.current = [];
          assistantKey.current = null;
          setBubbles([]);
          setReviewing(null);
          setReviewBubbles([]);
          setReviewError(null);
          refreshConversations(id);
        }),
      ),
      subscribe(() =>
        onChatState((ev) => {
          setState(ev);
          // `connecting` is the only unambiguous session boundary: it is
          // emitted once before every connect attempt, reconnects after an
          // error / character switch / session roll included, and by then the
          // previous socket is gone. `idle` and `error` deliberately don't
          // trigger this — the backend also emits those mid-session (on
          // `response.done` with the mic closed, and on non-fatal server
          // error events), where a transcription may still legitimately be
          // in flight and its placeholder must be kept.
          if (ev.state === "connecting") discardPendingUserBubbles();
        }),
      ),
      subscribe(() => onChatLevel(setLevel)),
      subscribe(() => onChatMic(setMicOpen)),
      subscribe(() => onChatRecording(setRecording)),
      subscribe(() =>
        onChatConversations((liveId) => {
          setActiveId(liveId);
          // Always the live character's list: `chat:character` fires before
          // the new session opens a conversation, so the ref is already
          // pointing at the right one.
          refreshConversations(characterIdRef.current);
        }),
      ),
      subscribe(() =>
        onChatTranscript((ev) => {
          setBubbles((prev) => {
            if (ev.role === "assistant") {
              if (assistantKey.current !== null) {
                const idx = prev.findIndex(
                  (b) => b.key === assistantKey.current,
                );
                if (idx !== -1) {
                  const next = [...prev];
                  next[idx] = { ...next[idx], text: ev.text };
                  if (ev.done) assistantKey.current = null;
                  return next;
                }
              }
              const key = nextKey.current++;
              if (!ev.done) assistantKey.current = key;
              return [...prev, { role: "assistant", text: ev.text, key }];
            }
            if (!ev.done) {
              // Placeholder reserving this turn's position; real text
              // follows once transcription completes.
              const key = nextKey.current++;
              pendingUserKeys.current.push(key);
              return [...prev, { role: "user", text: ev.text, key }];
            }
            const pendingKey = pendingUserKeys.current.shift();
            if (pendingKey !== undefined) {
              const idx = prev.findIndex((b) => b.key === pendingKey);
              if (idx !== -1) {
                const next = [...prev];
                next[idx] = { ...next[idx], text: ev.text };
                return next;
              }
            }
            return [
              ...prev,
              { role: "user", text: ev.text, key: nextKey.current++ },
            ];
          });
        }),
      ),
    ];
    return () => {
      restoreStale = true;
      cleanups.forEach((cleanup) => cleanup());
    };
  }, [refreshConversations]);

  // A conversation deleted or renamed elsewhere in the list must not leave a
  // stale header on the review pane.
  useEffect(() => {
    if (!reviewing) return;
    const fresh = conversations.find((c) => c.id === reviewing.id);
    if (!fresh) {
      setReviewing(null);
      setReviewBubbles([]);
    } else if (fresh.title !== reviewing.title) {
      setReviewing(fresh);
    }
  }, [conversations, reviewing]);

  // "开启新对话": the live session finalizes whatever it was writing to (named
  // and summarized into memory the same as any other end) and opens a fresh
  // row. The transcript is cleared right away rather than waiting on an
  // event for it — the old conversation is already safely in the history
  // list, so there is nothing left for `bubbles` to hold onto.
  function handleNewConversation() {
    setReviewError(null);
    setReviewing(null);
    setReviewBubbles([]);
    pendingUserKeys.current = [];
    assistantKey.current = null;
    setBubbles([]);
    ipc.newConversation();
  }

  async function openConversation(id: string | null) {
    setReviewError(null);
    if (id === null) {
      setReviewing(null);
      setReviewBubbles([]);
      return;
    }
    const summary = conversations.find((c) => c.id === id);
    if (!summary) return;
    setReviewing(summary);
    try {
      const messages = await ipc.getConversationMessages(id);
      setReviewBubbles(
        messages.map((m, i) => ({ role: m.role, text: m.text, key: i })),
      );
    } catch (e) {
      setReviewError(String(e));
      setReviewBubbles([]);
    }
  }

  // The mic stays open across multiple turns — the backend submits each one
  // automatically as you pause and lets you barge in on the AI just by
  // talking — so this is a toggle, not press-and-hold. `micOpen` itself is
  // driven entirely by the `chat:mic` event (not set optimistically here)
  // since the global hotkey can also flip it from outside this tab.
  function handleMicToggle() {
    // Opening the mic means the live conversation is what you want to see;
    // leaving a past one on screen would hide your own turns as you speak
    // them. Closing it leaves the review alone.
    if (!micOpen) openConversation(null);
    ipc.toggleTalking();
  }

  function toggleRecording() {
    if (recording === null) return;
    ipc.setRecording(!recording);
    setRecording(!recording);
  }

  const levelPct = Math.min(1, level * 4) * 100;
  const shown = reviewing ? reviewBubbles : bubbles;

  return (
    <div className="flex h-full text-neutral-100">
      <ConversationList
        conversations={conversations}
        activeId={activeId}
        selectedId={reviewing?.id ?? null}
        onSelect={openConversation}
        onChanged={() => refreshConversations(characterIdRef.current)}
        onNewConversation={handleNewConversation}
      />

      <div className="mx-auto flex h-full min-w-0 max-w-xl flex-1 flex-col p-6">
        <header className="flex items-center justify-between gap-2 pb-4">
          {reviewing ? (
            <>
              <div className="min-w-0">
                <p className="truncate text-sm text-neutral-300">
                  {reviewing.title || reviewing.preview}
                </p>
                <p className="text-xs text-neutral-500">
                  {t("history.viewing")}
                </p>
              </div>
              <button
                onClick={() => openConversation(null)}
                className={`${btn.outline} shrink-0 px-3 py-1.5 text-xs`}
              >
                {t("history.backToLive")}
              </button>
            </>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <span
                  className={`h-2.5 w-2.5 rounded-full ${STATE_COLOR[state.state]}`}
                />
                <span className="text-sm text-neutral-300">
                  {t(STATE_LABEL[state.state])}
                </span>
                {state.state === "error" && state.message && (
                  <span className="truncate text-xs text-red-400">
                    {state.message}
                  </span>
                )}
              </div>
              {recording !== null && (
                <button
                  onClick={toggleRecording}
                  title={t(
                    recording ? "chat.memory.onTitle" : "chat.memory.offTitle",
                  )}
                  className={`${btnBase} rounded-full px-2.5 py-1 text-xs ${
                    recording
                      ? "bg-neutral-800 text-neutral-300 hover:bg-neutral-700 hover:text-neutral-100"
                      : "bg-amber-500/20 text-amber-400 hover:bg-amber-500/30 hover:text-amber-300"
                  }`}
                >
                  {t(recording ? "chat.memory.on" : "chat.memory.off")}
                </button>
              )}
            </>
          )}
        </header>

        <div className="flex-1 space-y-3 overflow-y-auto">
          {reviewError && (
            <p role="alert" className="text-sm text-red-400">
              {t("history.loadFailed")}
            </p>
          )}
          {!reviewing && shown.length === 0 && (
            <p className="text-sm text-neutral-500">
              {hotkey ? t("chat.emptyWithHotkey", { hotkey }) : t("chat.empty")}
            </p>
          )}
          {shown.map((b) =>
            b.role === "user" && b.text === "" ? null : (
              <div
                key={b.key}
                className={`flex ${b.role === "user" ? "justify-end" : "justify-start"}`}
              >
                <div
                  className={`max-w-[80%] rounded-2xl px-4 py-2 text-sm ${
                    b.role === "user"
                      ? "bg-neutral-100 text-neutral-900"
                      : "bg-neutral-800 text-neutral-100"
                  }`}
                >
                  {b.text}
                </div>
              </div>
            ),
          )}
        </div>

        <div className="space-y-3 pt-4">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-neutral-800">
            <div
              className="h-full bg-emerald-400 transition-[width] duration-75"
              style={{ width: `${levelPct}%` }}
            />
          </div>
          <button
            onClick={handleMicToggle}
            className={`${btnBase} ${hoverGlow} w-full rounded-xl py-3 text-sm font-medium ${
              micOpen
                ? "bg-emerald-500 text-neutral-950 hover:bg-emerald-400 hover:shadow-emerald-500/25"
                : "bg-neutral-800 text-neutral-100 hover:bg-neutral-700 hover:shadow-black/40"
            }`}
          >
            {t(micOpen ? "chat.micOn" : "chat.micOff")}
          </button>
        </div>
      </div>
    </div>
  );
}
