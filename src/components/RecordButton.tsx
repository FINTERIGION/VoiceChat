import { useEffect, useRef, useState } from "react";
import { Mic, Square } from "lucide-react";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btnBase, hoverGlow } from "../lib/ui";

/** Matches `MAX_RECORD_SECS` in `voice/clone.rs`, past which nothing is kept. */
const MAX_RECORD_SECS = 60;

/** `75` → `1:15`. */
function formatSeconds(total: number): string {
  const s = Math.min(total, MAX_RECORD_SECS);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * Records a voice sample with the backend's recorder: one button that starts,
 * shows the time, and stops — by hand or at the cap. The Voice Studio's clone
 * tab uses it, and so does sharing a character whose cloned voice has no
 * sample kept.
 *
 * `onStart` runs once the microphone is open; `onRecorded` gets the result as
 * a WAV `data:` URI. `disabled` stops a new recording from starting, never one
 * already under way from being stopped.
 */
export default function RecordButton({
  onStart,
  onRecorded,
  onError,
  disabled = false,
}: {
  onStart?: () => void;
  onRecorded: (dataUri: string) => void;
  onError: (message: string) => void;
  disabled?: boolean;
}) {
  const t = useT();
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  // Read by the unmount cleanup, which must not re-run on every change.
  const recordingRef = useRef(false);
  // Set while `stopRecording` is in flight, so the timer hitting the cap and
  // a click on the button can't both stop the same recording.
  const stopping = useRef(false);

  useEffect(() => {
    recordingRef.current = recording;
    if (!recording) return;
    const started = Date.now();
    setElapsed(0);
    const timer = window.setInterval(
      () => setElapsed(Math.floor((Date.now() - started) / 1000)),
      250,
    );
    return () => window.clearInterval(timer);
  }, [recording]);

  // The backend keeps nothing past the cap, so carrying on would only look
  // like it is still listening.
  useEffect(() => {
    if (recording && elapsed >= MAX_RECORD_SECS) void stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, elapsed]);

  // Closing the dialog, or leaving the tab, mid-recording must not leave the
  // microphone capturing with nothing left on screen to stop it.
  useEffect(
    () => () => {
      if (recordingRef.current && !stopping.current) {
        ipc.stopRecording().catch(() => {});
      }
    },
    [],
  );

  async function stop() {
    if (stopping.current) return;
    stopping.current = true;
    try {
      onRecorded(await ipc.stopRecording());
    } catch (e) {
      onError(String(e));
    } finally {
      stopping.current = false;
      setRecording(false);
    }
  }

  async function toggle() {
    if (recording) {
      await stop();
      return;
    }
    try {
      await ipc.startRecording();
      setRecording(true);
      onStart?.();
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <button
      onClick={toggle}
      disabled={disabled && !recording}
      className={`${btnBase} ${hoverGlow} w-full gap-2 rounded-lg py-2.5 text-sm font-medium ${
        recording
          ? "bg-red-500 text-neutral-950 not-disabled:hover:bg-red-400 not-disabled:hover:shadow-red-500/25"
          : "bg-neutral-800 text-neutral-100 not-disabled:hover:bg-neutral-700 not-disabled:hover:shadow-black/40"
      }`}
    >
      {recording ? (
        <Square className="size-3.5 fill-current" />
      ) : (
        <Mic className="size-4" />
      )}
      {recording
        ? t("voice.clone.stop", {
            elapsed: formatSeconds(elapsed),
            max: formatSeconds(MAX_RECORD_SECS),
          })
        : t("voice.clone.start")}
    </button>
  );
}
