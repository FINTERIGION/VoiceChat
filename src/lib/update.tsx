import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ipc } from "./ipc";
import type { UpdateInfo } from "./types";

/**
 * Where the app stands with updates. `info` rides along through the
 * download so the page keeps showing what is being installed, and stays on
 * a failed install so it can be tried again without checking first.
 */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; info: UpdateInfo }
  | {
      kind: "downloading";
      info: UpdateInfo;
      downloaded: number;
      total: number | null;
    }
  | { kind: "installing"; info: UpdateInfo }
  | { kind: "failed"; message: string; info: UpdateInfo | null };

interface Updates {
  state: UpdateState;
  /** Looks for a newer release and reports the outcome, failures included. */
  check: () => Promise<void>;
  /** Installs what the last check found; the app closes and reopens. */
  install: () => Promise<void>;
}

/** The app lives in the tray for days at a time, so it looks again now and then. */
const RECHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const UpdateContext = createContext<Updates | null>(null);

/**
 * Shared by the banner the main window shows when a newer version turns up
 * and the About section in Settings, where it gets installed — so both
 * agree on what was found, and a download started from one shows on the
 * other.
 */
export function UpdateProvider({ children }: { children: ReactNode }) {
  // Mirrored in a ref so the callbacks below can read where things stand
  // without being rebuilt on every progress message.
  const stateRef = useRef<UpdateState>({ kind: "idle" });
  const [state, setState] = useState<UpdateState>(stateRef.current);
  const set = useCallback((next: UpdateState) => {
    stateRef.current = next;
    setState(next);
  }, []);

  const check = useCallback(async () => {
    const now = stateRef.current.kind;
    if (now === "checking" || now === "downloading" || now === "installing") {
      return;
    }
    set({ kind: "checking" });
    try {
      const info = await ipc.checkForUpdate();
      set(info ? { kind: "available", info } : { kind: "latest" });
    } catch (e) {
      set({ kind: "failed", message: String(e), info: null });
    }
  }, [set]);

  const install = useCallback(async () => {
    const now = stateRef.current;
    const info =
      now.kind === "available" || now.kind === "failed" ? now.info : null;
    if (!info) return;
    set({ kind: "downloading", info, downloaded: 0, total: null });
    try {
      await ipc.installUpdate((event) =>
        set(
          event.event === "progress"
            ? {
                kind: "downloading",
                info,
                downloaded: event.downloaded,
                total: event.total,
              }
            : { kind: "installing", info },
        ),
      );
    } catch (e) {
      set({ kind: "failed", message: String(e), info });
    }
  }, [set]);

  // Checks in the background: at launch, then every few hours. Nothing is
  // said unless it finds something — a failure here is the user's network
  // or GitHub having a moment, not something they asked about.
  useEffect(() => {
    // `tauri dev` runs whatever is checked out, which no release describes.
    if (import.meta.env.DEV) return;
    async function checkQuietly() {
      const before = stateRef.current;
      if (before.kind !== "idle" && before.kind !== "latest") return;
      try {
        const info = await ipc.checkForUpdate();
        // A check the user started meanwhile reports for itself.
        if (stateRef.current === before) {
          set(info ? { kind: "available", info } : { kind: "latest" });
        }
      } catch (e) {
        console.warn("background update check failed", e);
      }
    }
    void checkQuietly();
    const timer = window.setInterval(checkQuietly, RECHECK_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [set]);

  const value = useMemo(
    () => ({ state, check, install }),
    [state, check, install],
  );
  return (
    <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
  );
}

export function useUpdates(): Updates {
  const ctx = useContext(UpdateContext);
  if (!ctx) {
    throw new Error("useUpdates must be used inside <UpdateProvider>");
  }
  return ctx;
}
