import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Brain,
  KeyRound,
  Mic,
  MicOff,
} from "lucide-react";
import Avatar from "../components/Avatar";
import CharacterSwitcher from "../components/CharacterSwitcher";
import ConversationList from "../components/ConversationList";
import { formatHotkey } from "../lib/format";
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
  Character,
  ChatMessage,
  ChatStateEvent,
  ConversationSummary,
} from "../lib/types";
import { btn, btnBase, hoverGlow } from "../lib/ui";

interface Bubble {
  role: "user" | "assistant";
  text: string;
  key: number;
  /** A user turn that has been heard but not transcribed yet. */
  pending?: boolean;
}

/**
 * How close to the bottom (in px) still counts as "at the bottom" — so a
 * reader who scrolled up to reread something isn't yanked back down by
 * every new line, while one a few pixels off still follows along.
 */
const STICK_THRESHOLD_PX = 48;

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

const ONBOARDING_STEPS = [
  "onboarding.step1",
  "onboarding.step2",
  "onboarding.step3",
] as const satisfies readonly MessageKey[];

export default function Chat({
  active,
  onOpenSettings,
  onOpenCharacters,
}: {
  active: boolean;
  /** The first-run prompt's way to the API key field. */
  onOpenSettings: () => void;
  /** The character menu's "Manage characters…". */
  onOpenCharacters: () => void;
}) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const [state, setState] = useState<ChatStateEvent>({ state: "idle" });
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [level, setLevel] = useState(0);
  const [micOpen, setMicOpen] = useState(false);
  // `null` until the first `chat:recording` event, and again whenever the
  // current character has memory switched off — in both cases there is no
  // choice to offer, so the toggle is not rendered at all.
  const [recording, setRecording] = useState<boolean | null>(null);
  const [hotkey, setHotkey] = useState<string | null>(null);
  // `undefined` until first read, so the header doesn't flash "no
  // character" while the name is on its way.
  const [character, setCharacter] = useState<Character | null | undefined>(
    undefined,
  );
  const characterName = character?.name;
  // Whom the live session was talking to before the latest switch, when that
  // conversation had anything in it — it went into their history, which is
  // no longer the one on screen, so the transcript says where it went.
  const [endedWith, setEndedWith] = useState<string | null>(null);
  // Mirrors of state the subscriptions below read. They're registered once,
  // at mount, so the values they close over would never change.
  const characterRef = useRef<Character | null | undefined>(undefined);
  const hadTurns = useRef(false);
  // `null` until first read. Only a definite "no key" shows the first-run
  // prompt — not the moment before the answer comes back.
  const [apiKeyConfigured, setApiKeyConfigured] = useState<boolean | null>(
    null,
  );
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  // The conversation the live session is writing to, `null` while nothing is
  // connected.
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

  // Call after `refreshConversations`, which is what points the ref at `id`.
  const loadCharacter = useCallback((id: string | null) => {
    if (!id) {
      setCharacter(null);
      return;
    }
    ipc
      .listCharacters()
      .then((list) => {
        // Another switch may have landed while this was in flight.
        if (characterIdRef.current !== id) return;
        setCharacter(list.find((c) => c.id === id) ?? null);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    characterRef.current = character;
  }, [character]);

  useEffect(() => {
    // Covers remounting this tab (switching tabs unmounts it, see App.tsx)
    // while the mic was already open via the global hotkey — after this,
    // `chat:mic` keeps it in sync with hotkey presses made from anywhere.
    ipc.getMicOpen().then(setMicOpen);
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
    ipc.getCurrentCharacterId().then((id) => {
      refreshConversations(id);
      loadCharacter(id);
    });

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
          // The same id is an edit to the live character reconnecting it,
          // not a switch: its conversation is still in the list on screen.
          const previous = characterRef.current;
          setEndedWith(
            previous && previous.id !== id && hadTurns.current
              ? previous.name
              : null,
          );
          pendingUserKeys.current = [];
          assistantKey.current = null;
          setBubbles([]);
          setReviewing(null);
          setReviewBubbles([]);
          setReviewError(null);
          refreshConversations(id);
          loadCharacter(id);
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
              return [
                ...prev,
                { role: "user", text: ev.text, key, pending: true },
              ];
            }
            const pendingKey = pendingUserKeys.current.shift();
            if (pendingKey !== undefined) {
              const idx = prev.findIndex((b) => b.key === pendingKey);
              if (idx !== -1) {
                const next = [...prev];
                next[idx] = { ...next[idx], text: ev.text, pending: false };
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
  }, [refreshConversations, loadCharacter]);

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
    setEndedWith(null);
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

  // The hotkey, the API key and the character's name can all change on
  // other tabs while this one stays mounted in the background, and nothing
  // announces any of them — so they're re-read each time the tab comes back.
  useEffect(() => {
    if (!active) return;
    ipc.getHotkey().then(setHotkey);
    ipc
      .getSecretStatus()
      .then((s) => setApiKeyConfigured(s.configured))
      .catch(() => {});
    if (characterIdRef.current) loadCharacter(characterIdRef.current);
  }, [active, loadCharacter]);

  const needsKey = apiKeyConfigured === false;

  const levelPct = Math.min(1, level * 4) * 100;
  const shown = reviewing ? reviewBubbles : bubbles;

  useEffect(() => {
    hadTurns.current = bubbles.some((b) => b.text !== "");
  }, [bubbles]);

  function handleScroll() {
    const el = scroller.current;
    if (!el) return;
    stickToBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD_PX;
  }

  // Opening a different transcript — a past one, or back to the live one —
  // starts at its latest line, wherever the last one was left.
  useLayoutEffect(() => {
    stickToBottom.current = true;
  }, [reviewing?.id]);

  // Follows the conversation as it grows (a new bubble, or the reply
  // streaming into the last one), unless the reader has scrolled up. Also
  // re-run when the tab comes back into view: while hidden the list has no
  // layout to scroll, so anything that arrived meanwhile is caught up here.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && active && stickToBottom.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [shown, active]);

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
                <p className="truncate text-sm font-medium text-neutral-100">
                  {reviewing.title || reviewing.preview}
                </p>
                <p className="mt-0.5 truncate text-xs text-neutral-500">
                  {characterName ? `${characterName} · ` : ""}
                  {t("history.viewing")}
                </p>
              </div>
              <button
                onClick={() => openConversation(null)}
                className={`${btn.outline} shrink-0 gap-1.5 px-3 py-1.5 text-xs`}
              >
                <ArrowLeft className="size-3.5" />
                {t("history.backToLive")}
              </button>
            </>
          ) : (
            <>
              <div className="flex min-w-0 items-center gap-3">
                {character && (
                  <Avatar
                    id={character.id}
                    name={character.name}
                    avatarPath={character.avatar_path}
                    size="md"
                  />
                )}
                <div className="min-w-0">
                  <CharacterSwitcher
                    current={character}
                    onManage={onOpenCharacters}
                  />
                  <div className="mt-0.5 flex min-w-0 items-center gap-1.5">
                    <span
                      className={`size-2 shrink-0 rounded-full ${STATE_COLOR[state.state]}`}
                    />
                    <span className="shrink-0 text-xs text-neutral-400">
                      {t(STATE_LABEL[state.state])}
                    </span>
                    {state.state === "error" && state.message && (
                      // Clipped to fit the header; the whole message is on hover.
                      <span
                        title={state.message}
                        className="line-clamp-2 min-w-0 text-xs break-words text-red-400"
                      >
                        {state.message}
                      </span>
                    )}
                  </div>
                </div>
              </div>
              {recording !== null && (
                <button
                  onClick={toggleRecording}
                  title={t(
                    recording ? "chat.memory.onTitle" : "chat.memory.offTitle",
                  )}
                  className={`${btnBase} shrink-0 gap-1 rounded-full px-2.5 py-1 text-xs ${
                    recording
                      ? "bg-neutral-800 text-neutral-300 hover:bg-neutral-700 hover:text-neutral-100"
                      : "bg-amber-500/20 text-amber-400 hover:bg-amber-500/30 hover:text-amber-300"
                  }`}
                >
                  <Brain className="size-3.5" />
                  {t(recording ? "chat.memory.on" : "chat.memory.off")}
                </button>
              )}
            </>
          )}
        </header>

        <div
          ref={scroller}
          onScroll={handleScroll}
          className="flex-1 space-y-3 overflow-y-auto"
        >
          {reviewError && (
            <p role="alert" className="text-sm text-red-400">
              {t("history.loadFailed")}
            </p>
          )}
          {!reviewing && needsKey && (
            <section className="rounded-2xl border border-neutral-800 bg-neutral-900/60 p-5">
              <div className="flex items-center gap-2.5">
                <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-emerald-500/15 text-emerald-400">
                  <KeyRound className="size-4" />
                </span>
                <h2 className="text-base font-semibold">
                  {t("onboarding.title")}
                </h2>
              </div>
              <p className="mt-3 text-sm leading-relaxed text-neutral-400">
                {t("onboarding.body")}
              </p>
              <ol className="mt-4 space-y-2">
                {ONBOARDING_STEPS.map((key, i) => (
                  <li
                    key={key}
                    className="flex items-start gap-2.5 text-sm text-neutral-300"
                  >
                    <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-neutral-800 text-xs text-neutral-400">
                      {i + 1}
                    </span>
                    {t(key)}
                  </li>
                ))}
              </ol>
              <button
                onClick={onOpenSettings}
                className={`${btn.primary} mt-5 gap-1.5 px-4 py-2 text-sm font-medium`}
              >
                {t("onboarding.cta")}
                <ArrowRight className="size-4" />
              </button>
            </section>
          )}
          {!reviewing && endedWith && (
            <p className="flex items-center gap-3 text-xs text-neutral-500 before:h-px before:flex-1 before:bg-neutral-800 after:h-px after:flex-1 after:bg-neutral-800">
              {t("chat.switchedNotice", { name: endedWith })}
            </p>
          )}
          {!reviewing && !needsKey && shown.length === 0 && (
            <p className="text-sm text-neutral-500">
              {hotkey
                ? t("chat.emptyWithHotkey", { hotkey: formatHotkey(hotkey) })
                : t("chat.empty")}
            </p>
          )}
          {shown.map((b) => {
            // Heard but not yet transcribed: a quiet stand-in keeps the turn
            // visible in its place, so speaking doesn't look like it went
            // nowhere while the words are on their way.
            if (b.pending && b.text === "") {
              return (
                <div key={b.key} className="flex justify-end">
                  <div className="animate-pulse rounded-2xl border border-dashed border-neutral-700 px-4 py-2 text-sm text-neutral-500">
                    {t("chat.transcribing")}
                  </div>
                </div>
              );
            }
            // A turn transcribed as nothing at all (a cough, a door).
            if (b.role === "user" && b.text === "") return null;
            return (
              <div
                key={b.key}
                className={`flex ${b.role === "user" ? "justify-end" : "justify-start"}`}
              >
                <div
                  className={`max-w-[80%] rounded-2xl px-4 py-2 text-sm whitespace-pre-wrap break-words ${
                    b.role === "user"
                      ? "bg-neutral-100 text-neutral-900"
                      : "bg-neutral-800 text-neutral-100"
                  }`}
                >
                  {b.text}
                </div>
              </div>
            );
          })}
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
            // Opening the mic without a key can only end in an error; the
            // prompt above says what to do instead. Closing always works.
            disabled={needsKey && !micOpen}
            title={needsKey && !micOpen ? t("chat.needApiKey") : undefined}
            className={`${btnBase} ${hoverGlow} w-full gap-2 rounded-xl py-3 text-sm font-medium ${
              micOpen
                ? "bg-emerald-500 text-neutral-950 not-disabled:hover:bg-emerald-400 not-disabled:hover:shadow-emerald-500/25"
                : "bg-neutral-800 text-neutral-100 not-disabled:hover:bg-neutral-700 not-disabled:hover:shadow-black/40"
            }`}
          >
            {micOpen ? <MicOff className="size-4" /> : <Mic className="size-4" />}
            {t(micOpen ? "chat.micOn" : "chat.micOff")}
          </button>
        </div>
      </div>
    </div>
  );
}
