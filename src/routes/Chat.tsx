import { useEffect, useRef, useState } from "react";
import {
  ipc,
  onChatCharacter,
  onChatLevel,
  onChatMic,
  onChatRecording,
  onChatState,
  onChatTranscript,
  subscribe,
} from "../lib/ipc";
import type { ChatStateEvent } from "../lib/types";

interface Bubble {
  role: "user" | "assistant";
  text: string;
  key: number;
}

const STATE_LABEL: Record<ChatStateEvent["state"], string> = {
  idle: "空闲",
  connecting: "连接中",
  listening: "聆听中",
  thinking: "思考中",
  speaking: "说话中",
  error: "出错了",
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
  const [state, setState] = useState<ChatStateEvent>({ state: "idle" });
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [level, setLevel] = useState(0);
  const [micOpen, setMicOpen] = useState(false);
  // `null` until the first `chat:recording` event, and again whenever the
  // current character has memory switched off — in both cases there is no
  // choice to offer, so the toggle is not rendered at all.
  const [recording, setRecording] = useState<boolean | null>(null);
  const [hotkey, setHotkey] = useState<string | null>(null);
  const nextKey = useRef(0);
  const assistantKey = useRef<number | null>(null);
  // Keys of user bubbles reserved (via the empty, `done: false` placeholder
  // emitted at speech_stopped) but not yet filled in by the async
  // transcription result. FIFO since transcriptions resolve in the order
  // their turns were committed.
  const pendingUserKeys = useRef<number[]>([]);

  useEffect(() => {
    // Covers remounting this tab (switching tabs unmounts it, see App.tsx)
    // while the mic was already open via the global hotkey — after this,
    // `chat:mic` keeps it in sync with hotkey presses made from anywhere.
    ipc.getMicOpen().then(setMicOpen);
    ipc.getHotkey().then(setHotkey);

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
        onChatCharacter(() => {
          // A different character is now live — its conversation has no
          // relation to whatever was on screen, and there is no backend
          // history to reload, so just clear the transcript.
          pendingUserKeys.current = [];
          assistantKey.current = null;
          setBubbles([]);
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
    return () => cleanups.forEach((cleanup) => cleanup());
  }, []);

  // The mic stays open across multiple turns — the backend submits each one
  // automatically as you pause and lets you barge in on the AI just by
  // talking — so this is a toggle, not press-and-hold. `micOpen` itself is
  // driven entirely by the `chat:mic` event (not set optimistically here)
  // since the global hotkey can also flip it from outside this tab.
  function handleMicToggle() {
    ipc.toggleTalking();
  }

  function toggleRecording() {
    if (recording === null) return;
    ipc.setRecording(!recording);
    setRecording(!recording);
  }

  const levelPct = Math.min(1, level * 4) * 100;

  return (
    <div className="mx-auto flex h-full max-w-xl flex-col p-6 text-neutral-100">
      <header className="flex items-center justify-between gap-2 pb-4">
        <div className="flex items-center gap-2">
          <span
            className={`h-2.5 w-2.5 rounded-full ${STATE_COLOR[state.state]}`}
          />
          <span className="text-sm text-neutral-300">
            {STATE_LABEL[state.state]}
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
            title={recording ? "本次对话会记入长期记忆" : "本次对话不会记入长期记忆"}
            className={`rounded-full px-2.5 py-1 text-xs ${
              recording
                ? "bg-neutral-800 text-neutral-300"
                : "bg-amber-500/20 text-amber-400"
            }`}
          >
            {recording ? "记忆：开" : "本次不记录"}
          </button>
        )}
      </header>

      <div className="flex-1 space-y-3 overflow-y-auto">
        {bubbles.length === 0 && (
          <p className="text-sm text-neutral-500">
            点击下方按钮{hotkey ? `或按 ${hotkey}` : ""}
            开启麦克风，说完自动发送；AI 说话时直接开口就能打断…
          </p>
        )}
        {bubbles.map((b) =>
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
          className={`w-full select-none rounded-xl py-3 text-sm font-medium ${
            micOpen
              ? "bg-emerald-500 text-neutral-950"
              : "bg-neutral-800 text-neutral-100"
          }`}
        >
          {micOpen ? "点击关闭麦克风" : "点击开始对话"}
        </button>
      </div>
    </div>
  );
}
