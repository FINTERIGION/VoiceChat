import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Captions,
  Check,
  CircleAlert,
  CircleCheck,
  DatabaseBackup,
  Download,
  Eye,
  EyeOff,
  Info,
  Keyboard,
  Library,
  LoaderCircle,
  Mic,
  Plug,
  RefreshCw,
  SlidersHorizontal,
  Trash2,
  Upload,
  type LucideIcon,
} from "lucide-react";
import ConfirmDialog from "../components/ConfirmDialog";
import Field from "../components/Field";
import {
  isUiLanguage,
  UI_LANGUAGES,
  useI18n,
  useT,
  type MessageKey,
  type UiLanguage,
} from "../lib/i18n";
import { formatDateTime, formatHotkey } from "../lib/format";
import { ipc } from "../lib/ipc";
import { useUpdates } from "../lib/update";
import type {
  ConnectionSettings,
  ManagedVoice,
  RegionOption,
  SecretStatus,
  SubtitleSettings,
  VadSettings,
} from "../lib/types";
import { btn, field, interactive } from "../lib/ui";

export type SettingsSection =
  | "general"
  | "connection"
  | "conversation"
  | "subtitle"
  | "voices"
  | "backup"
  | "about";

const SECTIONS: { id: SettingsSection; label: MessageKey; icon: LucideIcon }[] =
  [
    { id: "general", label: "settings.nav.general", icon: SlidersHorizontal },
    { id: "connection", label: "settings.nav.connection", icon: Plug },
    { id: "conversation", label: "settings.nav.conversation", icon: Mic },
    { id: "subtitle", label: "settings.subtitle.heading", icon: Captions },
    { id: "voices", label: "settings.voices.heading", icon: Library },
    { id: "backup", label: "settings.backup.heading", icon: DatabaseBackup },
    { id: "about", label: "settings.about.heading", icon: Info },
  ];

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "ShiftLeft",
  "ShiftRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
]);

/** How long a slider waits after the last movement before saving. */
const SLIDER_SAVE_DELAY_MS = 400;

type SaveStatus =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "saved" }
  | { kind: "error"; message: string };

/**
 * How every setting on this page is saved: as soon as it changes, one save
 * at a time and in order, with the outcome shown the same way everywhere —
 * a spinner, then a tick that fades, or the reason it failed. Failures carry
 * the backend's message, since some are validation the user has to act on
 * (a workspace id that isn't one, a hotkey another app already holds).
 *
 * `run` resolves to whether that save went through, for a caller that has
 * to put its control back when it didn't. `settled` resolves once
 * everything queued so far has finished, to whether the last of it
 * succeeded.
 */
function useSaver() {
  const [status, setStatus] = useState<SaveStatus>({ kind: "idle" });
  const queue = useRef<Promise<boolean>>(Promise.resolve(true));
  const latest = useRef(0);
  const fade = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(fade.current), []);

  const run = useCallback((save: () => Promise<unknown>) => {
    const id = ++latest.current;
    window.clearTimeout(fade.current);
    setStatus({ kind: "saving" });
    const done = queue.current
      .then(() => save())
      .then(
        () => true,
        (e: unknown) => {
          // Only the newest save gets to report: an older one finishing
          // mustn't show "saved" while a later one is still being written.
          if (id === latest.current) {
            setStatus({ kind: "error", message: String(e) });
          }
          return false;
        },
      )
      .then((ok) => {
        if (ok && id === latest.current) {
          setStatus({ kind: "saved" });
          fade.current = window.setTimeout(
            () => setStatus({ kind: "idle" }),
            1500,
          );
        }
        return ok;
      });
    queue.current = done;
    return done;
  }, []);

  const settled = useCallback(() => queue.current, []);

  return { status, run, settled };
}

/**
 * Reads what a section shows, once, when it opens. Each section reads its
 * own, so a section always shows what is stored now — including after a
 * restore from backup has rewritten everything behind the page's back.
 */
function useLoaded<T>(load: () => Promise<T>) {
  // `undefined` until the read comes back — kept apart from a stored `null`,
  // which is a real value (no hotkey set).
  const [value, setValue] = useState<T | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    load().then(
      (v) => live && setValue(v),
      (e) => live && setError(String(e)),
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return [value, setValue, error] as const;
}

export default function Settings({
  section,
  onSectionChange,
}: {
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
}) {
  const t = useT();
  const { state: update } = useUpdates();
  // Shared by the sections that depend on there being a key, and by the
  // nav, which flags the Connection section while there isn't one.
  const [status, setStatus] = useState<SecretStatus | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await ipc.getSecretStatus());
    } catch (e) {
      console.error("couldn't read the API key status", e);
    }
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];

  return (
    <div className="flex h-full text-neutral-100">
      <nav
        aria-label={t("settings.title")}
        className="flex w-48 shrink-0 flex-col border-r border-neutral-800 px-2 py-4"
      >
        <h1 className="px-3 pb-3 text-xs font-medium uppercase tracking-wide text-neutral-500">
          {t("settings.title")}
        </h1>
        <ul className="space-y-0.5">
          {SECTIONS.map(({ id, label, icon: Icon }) => {
            const selected = id === section;
            const missingKey =
              id === "connection" && status !== null && !status.configured;
            const updateReady = id === "about" && update.kind === "available";
            return (
              <li key={id}>
                <button
                  onClick={() => onSectionChange(id)}
                  aria-current={selected ? "page" : undefined}
                  className={`${interactive} flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm ${
                    selected
                      ? "bg-neutral-800 text-neutral-100"
                      : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
                  }`}
                >
                  <Icon className="size-4 shrink-0" />
                  <span className="min-w-0 flex-1 truncate">{t(label)}</span>
                  {missingKey && (
                    <span
                      role="img"
                      title={t("settings.nav.connectionMissing")}
                      aria-label={t("settings.nav.connectionMissing")}
                      className="size-2 shrink-0 rounded-full bg-amber-400"
                    />
                  )}
                  {updateReady && (
                    <span
                      role="img"
                      title={t("settings.nav.updateAvailable")}
                      aria-label={t("settings.nav.updateAvailable")}
                      className="size-2 shrink-0 rounded-full bg-emerald-400"
                    />
                  )}
                </button>
              </li>
            );
          })}
        </ul>
        <p className="mt-auto flex items-center gap-1.5 px-3 pt-4 text-xs text-neutral-500">
          <Check className="size-3.5 shrink-0" />
          {t("settings.autosave")}
        </p>
      </nav>

      {/* Keyed by section so each one opens scrolled to its top. */}
      <div key={section} className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-xl space-y-5 p-8">
          <h2 className="text-xl font-semibold">{t(current.label)}</h2>
          {section === "general" && <GeneralSection />}
          {section === "connection" && (
            <ConnectionSection status={status} refreshStatus={refreshStatus} />
          )}
          {section === "conversation" && <ConversationSection />}
          {section === "subtitle" && <SubtitleSection />}
          {section === "voices" && <VoicesSection status={status} />}
          {section === "backup" && (
            <BackupSection status={status} refreshStatus={refreshStatus} />
          )}
          {section === "about" && <AboutSection />}
        </div>
      </div>
    </div>
  );
}

// ---- Shared pieces ---------------------------------------------------------

function Card({
  title,
  description,
  status,
  action,
  children,
}: {
  title?: string;
  description?: string;
  status?: SaveStatus;
  action?: ReactNode;
  children: ReactNode;
}) {
  const hasHeader = title || description || status || action;
  return (
    <section className="space-y-4 rounded-xl border border-neutral-800 bg-neutral-900/40 p-5">
      {hasHeader && (
        <header className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            {title && (
              <h3 className="text-sm font-medium text-neutral-100">{title}</h3>
            )}
            {description && (
              <p
                className={`text-xs leading-relaxed text-neutral-500 ${title ? "mt-1" : ""}`}
              >
                {description}
              </p>
            )}
          </div>
          {(status || action) && (
            <div className="flex shrink-0 items-center gap-2">
              {status && <SaveIndicator status={status} />}
              {action}
            </div>
          )}
        </header>
      )}
      {children}
    </section>
  );
}

function SaveIndicator({ status }: { status: SaveStatus }) {
  const t = useT();
  return (
    <span aria-live="polite" className="flex items-center gap-1 text-xs">
      {status.kind === "saving" && (
        <span className="flex items-center gap-1 text-neutral-500">
          <LoaderCircle className="size-3.5 animate-spin" />
          {t("common.saving")}
        </span>
      )}
      {status.kind === "saved" && (
        <span className="flex items-center gap-1 text-emerald-400">
          <Check className="size-3.5" />
          {t("common.saved")}
        </span>
      )}
      {status.kind === "error" && (
        <span className="flex items-center gap-1 text-red-400">
          <CircleAlert className="size-3.5" />
          {t("common.saveFailed")}
        </span>
      )}
    </span>
  );
}

/** The reason under a card whose indicator says "couldn't save". */
function SaveError({ status }: { status: SaveStatus }) {
  if (status.kind !== "error") return null;
  return (
    <p role="alert" className="text-xs leading-relaxed text-red-400">
      {status.message}
    </p>
  );
}

function Loading({ error }: { error: string | null }) {
  const t = useT();
  return error ? (
    <p role="alert" className="text-sm text-red-400">
      {error}
    </p>
  ) : (
    <p className="text-sm text-neutral-500">{t("common.loading")}</p>
  );
}

function Outcome({ ok, children }: { ok: boolean; children: ReactNode }) {
  const Icon = ok ? CircleCheck : CircleAlert;
  return (
    <p
      role={ok ? "status" : "alert"}
      className={`flex items-start gap-1.5 text-sm break-all ${ok ? "text-emerald-400" : "text-red-400"}`}
    >
      <Icon className="mt-0.5 size-4 shrink-0" />
      <span>{children}</span>
    </p>
  );
}

// ---- General ---------------------------------------------------------------

function GeneralSection() {
  const { lang, setLang, t } = useI18n();
  const saver = useSaver();

  return (
    <Card
      title={t("settings.language.heading")}
      description={t("settings.language.hint")}
      status={saver.status}
    >
      <select
        value={lang}
        aria-label={t("settings.language.heading")}
        onChange={(e) => {
          const next = e.target.value as UiLanguage;
          void saver.run(() => setLang(next));
        }}
        className={`${field} w-full max-w-xs`}
      >
        {UI_LANGUAGES.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </select>
      <SaveError status={saver.status} />
    </Card>
  );
}

// ---- Connection ------------------------------------------------------------

type TestState =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "success" }
  | { kind: "error"; message: string };

function ConnectionSection({
  status,
  refreshStatus,
}: {
  status: SecretStatus | null;
  refreshStatus: () => Promise<void>;
}) {
  const { lang, t } = useI18n();
  const keySaver = useSaver();
  const endpointSaver = useSaver();
  const [keyInput, setKeyInput] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const keyField = useRef<HTMLInputElement>(null);
  const [endpoint, , endpointError] = useLoaded(ipc.getConnectionSettings);
  const [workspaceId, setWorkspaceId] = useState("");
  const [region, setRegion] = useState("");
  const [regions, setRegions] = useState<RegionOption[]>([]);
  // What the backend holds, so leaving the workspace box without having
  // changed it doesn't write the same value back.
  const savedEndpoint = useRef<ConnectionSettings | null>(null);
  const [test, setTest] = useState<TestState>({ kind: "idle" });

  useEffect(() => {
    if (!endpoint) return;
    savedEndpoint.current = endpoint;
    setWorkspaceId(endpoint.workspace_id ?? "");
    setRegion(endpoint.region ?? "");
  }, [endpoint]);

  // Region labels are produced by the backend in the current display
  // language, so they have to be re-fetched whenever that changes.
  useEffect(() => {
    ipc.listRegions().then(setRegions);
  }, [lang]);

  // Arriving here without a key — from the first-run prompt on the Chat
  // tab, most likely — the key box is the one thing to do.
  useEffect(() => {
    if (status && !status.configured) keyField.current?.focus();
  }, [status]);

  // The key is the one thing here that isn't saved as it changes: it's
  // pasted in whole rather than edited, and writing each keystroke's
  // prefix to the credential store would be both pointless and alarming.
  async function saveKey() {
    const key = keyInput.trim();
    if (!key) return;
    const ok = await keySaver.run(() => ipc.setApiKey(key));
    if (ok) {
      setKeyInput("");
      setShowKey(false);
      setTest({ kind: "idle" });
      await refreshStatus();
    }
  }

  async function clearKey() {
    const ok = await keySaver.run(() => ipc.clearApiKey());
    setConfirmingClear(false);
    if (ok) {
      setTest({ kind: "idle" });
      await refreshStatus();
    }
  }

  function saveEndpoint(next: ConnectionSettings) {
    const saved = savedEndpoint.current;
    if (
      saved &&
      saved.workspace_id === next.workspace_id &&
      saved.region === next.region
    ) {
      return;
    }
    void endpointSaver.run(() => ipc.setConnectionSettings(next)).then((ok) => {
      if (ok) {
        savedEndpoint.current = next;
        // Whatever the last test said was about the endpoint before this.
        setTest({ kind: "idle" });
      }
    });
  }

  async function runTest() {
    setTest({ kind: "pending" });
    // Clicking here takes focus from the workspace box, which saves it — so
    // an edit may be on its way to the backend right now, and testing
    // before it lands would test the endpoint from before it.
    if (!(await endpointSaver.settled())) {
      setTest({ kind: "error", message: t("settings.connectivity.fixFirst") });
      return;
    }
    try {
      await ipc.testConnectivity();
      setTest({ kind: "success" });
    } catch (e) {
      setTest({ kind: "error", message: String(e) });
    }
  }

  return (
    <>
      <Card
        title={t("settings.apiKey.heading")}
        description={t("settings.apiKey.hint")}
        status={keySaver.status}
      >
        {status === null ? (
          <p className="text-sm text-neutral-500">{t("common.loading")}</p>
        ) : status.configured ? (
          <p className="flex items-center gap-1.5 text-sm text-emerald-400">
            <CircleCheck className="size-4" />
            {t("settings.apiKey.configured", { tail: status.tail ?? "" })}
          </p>
        ) : (
          <p className="flex items-center gap-1.5 text-sm text-amber-400">
            <CircleAlert className="size-4" />
            {t("settings.apiKey.missing")}
          </p>
        )}

        <div className="flex gap-2">
          <div className="relative flex-1">
            <input
              ref={keyField}
              type={showKey ? "text" : "password"}
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void saveKey();
              }}
              placeholder="sk-..."
              aria-label={t("settings.apiKey.heading")}
              autoComplete="off"
              spellCheck={false}
              className={`${field} w-full pr-10 font-mono`}
            />
            <button
              type="button"
              onClick={() => setShowKey((on) => !on)}
              title={t(showKey ? "settings.apiKey.hide" : "settings.apiKey.show")}
              aria-label={t(
                showKey ? "settings.apiKey.hide" : "settings.apiKey.show",
              )}
              aria-pressed={showKey}
              className={`${btn.quiet} absolute inset-y-1 right-1 w-8`}
            >
              {showKey ? (
                <EyeOff className="size-4" />
              ) : (
                <Eye className="size-4" />
              )}
            </button>
          </div>
          <button
            onClick={saveKey}
            disabled={!keyInput.trim()}
            className={`${btn.primary} px-4 py-2 text-sm font-medium`}
          >
            {t("common.save")}
          </button>
          {status?.configured && (
            <button
              onClick={() => setConfirmingClear(true)}
              className={`${btn.dangerOutline} px-3 py-2 text-sm`}
            >
              {t("settings.apiKey.clear")}
            </button>
          )}
        </div>
        <SaveError status={keySaver.status} />
      </Card>

      <Card
        title={t("settings.optional.heading")}
        description={t("settings.optional.hint")}
        status={endpointSaver.status}
      >
        {endpoint === undefined ? (
          <Loading error={endpointError} />
        ) : (
          <div className="grid grid-cols-2 gap-3">
            <Field label={t("settings.optional.workspaceId")}>
              <input
                value={workspaceId}
                onChange={(e) => setWorkspaceId(e.target.value)}
                onBlur={() =>
                  saveEndpoint({
                    workspace_id: workspaceId.trim() || null,
                    region: region || null,
                  })
                }
                onKeyDown={(e) => {
                  if (e.key === "Enter") e.currentTarget.blur();
                }}
                placeholder={t("settings.optional.workspacePlaceholder")}
                spellCheck={false}
                className={`${field} w-full`}
              />
            </Field>
            <Field label={t("settings.optional.region")}>
              <select
                value={region}
                onChange={(e) => {
                  setRegion(e.target.value);
                  saveEndpoint({
                    workspace_id: workspaceId.trim() || null,
                    region: e.target.value || null,
                  });
                }}
                className={`${field} w-full`}
              >
                <option value="">
                  {t("settings.optional.regionDefault", {
                    label:
                      regions.find((r) => r.id === "cn-beijing")?.label ??
                      t("settings.optional.regionFallback"),
                  })}
                </option>
                {regions
                  .filter((r) => r.id !== "cn-beijing")
                  .map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.label}
                    </option>
                  ))}
              </select>
            </Field>
          </div>
        )}
        <SaveError status={endpointSaver.status} />
      </Card>

      <Card
        title={t("settings.connectivity.heading")}
        description={t("settings.connectivity.hint")}
      >
        <button
          onClick={runTest}
          disabled={test.kind === "pending" || !status?.configured}
          title={status?.configured ? undefined : t("chat.needApiKey")}
          className={`${btn.solid} gap-2 px-4 py-2 text-sm font-medium`}
        >
          {test.kind === "pending" ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <Plug className="size-4" />
          )}
          {test.kind === "pending"
            ? t("settings.connectivity.testing")
            : t("settings.connectivity.test")}
        </button>
        {test.kind === "success" && (
          <Outcome ok>{t("settings.connectivity.ok")}</Outcome>
        )}
        {test.kind === "error" && (
          <Outcome ok={false}>{test.message}</Outcome>
        )}
      </Card>

      {confirmingClear && (
        <ConfirmDialog
          title={t("settings.apiKey.clearTitle")}
          body={t("settings.apiKey.clearBody")}
          confirmLabel={t("settings.apiKey.clear")}
          busy={keySaver.status.kind === "saving"}
          onConfirm={clearKey}
          onCancel={() => setConfirmingClear(false)}
        />
      )}
    </>
  );
}

// ---- Conversation ----------------------------------------------------------

function ConversationSection() {
  const t = useT();
  const vadSaver = useSaver();
  const hotkeySaver = useSaver();
  const [vad, setVad, vadError] = useLoaded(ipc.getVadSettings);
  // The newest slider values, read by the debounced save and by the flush
  // on the way out — both run after the render that set them.
  const latestVad = useRef<VadSettings | null>(null);
  const pendingVadSave = useRef<number | undefined>(undefined);
  const [hotkey, setHotkey, hotkeyError] = useLoaded(ipc.getHotkey);
  const [capturing, setCapturing] = useState(false);

  // A slider moved in the last moment before the page closes is written
  // straight away rather than dropped with the timer.
  useEffect(
    () => () => {
      if (pendingVadSave.current === undefined) return;
      window.clearTimeout(pendingVadSave.current);
      if (latestVad.current) {
        ipc.setVadSettings(latestVad.current).catch(() => {});
      }
    },
    [],
  );

  // Dragging a slider fires a change for every step it passes, so the save
  // waits for it to come to rest.
  function changeVad(patch: Partial<VadSettings>) {
    const base = latestVad.current ?? vad;
    if (!base) return;
    const next = { ...base, ...patch };
    latestVad.current = next;
    setVad(next);
    window.clearTimeout(pendingVadSave.current);
    pendingVadSave.current = window.setTimeout(() => {
      pendingVadSave.current = undefined;
      void vadSaver.run(() => ipc.setVadSettings(next));
    }, SLIDER_SAVE_DELAY_MS);
  }

  async function applyHotkey(next: string | null, previous: string | null) {
    if (next === previous) return;
    setHotkey(next);
    const ok = await hotkeySaver.run(() => ipc.setHotkey(next));
    // Registering can fail — another app may already hold the combination —
    // and then the old hotkey is still the one in effect.
    if (!ok) setHotkey(previous);
  }

  useEffect(() => {
    if (!capturing) return;
    const previous = hotkey ?? null;
    function onKeyDown(e: KeyboardEvent) {
      e.preventDefault();
      // A bare modifier press isn't a complete combo yet — keep waiting.
      if (MODIFIER_CODES.has(e.code)) return;
      // Escape on its own backs out and keeps the hotkey as it was.
      if (
        e.code === "Escape" &&
        !e.ctrlKey &&
        !e.shiftKey &&
        !e.altKey &&
        !e.metaKey
      ) {
        setCapturing(false);
        return;
      }
      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      if (e.metaKey) parts.push("Super");
      parts.push(e.code);
      setCapturing(false);
      void applyHotkey(parts.join("+"), previous);
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing]);

  return (
    <>
      <Card
        title={t("settings.vad.heading")}
        description={t("settings.vad.hint")}
        status={vadSaver.status}
      >
        {vad === undefined ? (
          <Loading error={vadError} />
        ) : (
          <div className="grid grid-cols-2 gap-5">
            <Field
              label={t("settings.vad.threshold", {
                value: vad.threshold.toFixed(2),
              })}
            >
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={vad.threshold}
                onChange={(e) =>
                  changeVad({ threshold: Number(e.target.value) })
                }
              />
            </Field>
            <Field label={t("settings.vad.silence", { ms: vad.silence_ms })}>
              <input
                type="range"
                min={200}
                max={6000}
                step={100}
                value={vad.silence_ms}
                onChange={(e) =>
                  changeVad({ silence_ms: Number(e.target.value) })
                }
              />
            </Field>
          </div>
        )}
        <SaveError status={vadSaver.status} />
      </Card>

      <Card
        title={t("settings.hotkey.heading")}
        description={t("settings.hotkey.hint")}
        status={hotkeySaver.status}
      >
        {hotkey === undefined ? (
          <Loading error={hotkeyError} />
        ) : (
          <div className="flex items-center gap-2">
            <button
              // Clicking again while it waits is the mouse's way back out.
              onClick={() => setCapturing((on) => !on)}
              aria-pressed={capturing}
              className={`${btn.outline} min-w-[14rem] justify-start gap-2 bg-neutral-900 px-3 py-2 text-sm`}
            >
              <Keyboard className="size-4 shrink-0 text-neutral-500" />
              {capturing ? (
                <span className="text-neutral-400">
                  {t("settings.hotkey.capturing")}
                </span>
              ) : hotkey ? (
                <span className="font-medium">{formatHotkey(hotkey)}</span>
              ) : (
                <span className="text-neutral-500">{t("common.notSet")}</span>
              )}
            </button>
            <button
              onClick={() => applyHotkey(null, hotkey ?? null)}
              disabled={!hotkey}
              className={`${btn.outline} px-3 py-2 text-sm`}
            >
              {t("settings.hotkey.disable")}
            </button>
          </div>
        )}
        <SaveError status={hotkeySaver.status} />
      </Card>
    </>
  );
}

// ---- Subtitle --------------------------------------------------------------

function SubtitleSection() {
  const t = useT();
  const saver = useSaver();
  const [settings, setSettings, loadError] = useLoaded(ipc.getSubtitleSettings);
  const [adjusting, setAdjusting] = useState(false);
  // Read at cleanup time rather than closed over, so leaving this section
  // mid-adjustment still turns click-through back on — the effect that
  // reads it must not re-run (and so re-register its cleanup) on every
  // toggle, which a dependency array here would cause.
  const adjustingRef = useRef(false);

  useEffect(() => {
    adjustingRef.current = adjusting;
  }, [adjusting]);

  // Leaving this section mid-adjustment must not leave the subtitle window
  // stuck non-click-through — nothing else offers a way back to "调整位置"
  // once these controls are gone.
  useEffect(
    () => () => {
      if (adjustingRef.current) void ipc.setSubtitleAdjusting(false);
    },
    [],
  );

  // Both switches take effect at once — the window itself appearing or
  // disappearing, or the second line showing up — like every other setting
  // on this page.
  async function update(next: SubtitleSettings) {
    if (!settings) return;
    const previous = settings;
    setSettings(next);
    if (!next.enabled && adjusting) {
      setAdjusting(false);
      void ipc.setSubtitleAdjusting(false);
    }
    const ok = await saver.run(() => ipc.setSubtitleSettings(next));
    // Revert to what's actually in effect rather than leaving the controls
    // showing a state the backend rejected.
    if (!ok) setSettings(previous);
  }

  function toggleAdjusting() {
    const next = !adjusting;
    setAdjusting(next);
    void ipc.setSubtitleAdjusting(next);
  }

  return (
    <Card description={t("settings.subtitle.hint")} status={saver.status}>
      {settings === undefined ? (
        <Loading error={loadError} />
      ) : (
        <div className="space-y-3">
          <label className="flex items-center gap-2 text-sm text-neutral-200">
            <input
              type="checkbox"
              checked={settings.enabled}
              onChange={(e) =>
                update({ ...settings, enabled: e.target.checked })
              }
            />
            {t("settings.subtitle.enable")}
          </label>
          {/* Indented under the main switch: these only exist while it's on. */}
          {settings.enabled && (
            <div className="ml-6 space-y-3">
              <label className="flex items-start gap-2 text-sm text-neutral-200">
                <input
                  type="checkbox"
                  checked={settings.translate}
                  onChange={(e) =>
                    update({ ...settings, translate: e.target.checked })
                  }
                  className="mt-0.5"
                />
                <span>
                  {t("settings.subtitle.translate")}
                  <span className="block text-xs text-neutral-500">
                    {t("settings.subtitle.translateHint")}
                  </span>
                </span>
              </label>
              <div className="flex items-center gap-3">
                <button
                  onClick={toggleAdjusting}
                  aria-pressed={adjusting}
                  className={`${adjusting ? btn.primary : btn.outline} px-3 py-2 text-sm`}
                >
                  {t(
                    adjusting
                      ? "settings.subtitle.adjustDone"
                      : "settings.subtitle.adjust",
                  )}
                </button>
                {adjusting && (
                  <span className="text-xs text-neutral-500">
                    {t("settings.subtitle.adjustHint")}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      )}
      <SaveError status={saver.status} />
    </Card>
  );
}

// ---- Voice library ---------------------------------------------------------

function VoicesSection({ status }: { status: SecretStatus | null }) {
  const { lang, t } = useI18n();
  const [voices, setVoices] = useState<ManagedVoice[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      setVoices(await ipc.listVoices());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (status?.configured) refresh();
  }, [status?.configured]);

  async function handleDelete(voiceId: string) {
    setDeletingId(voiceId);
    setError(null);
    try {
      await ipc.deleteVoice(voiceId);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeletingId(null);
      // Dismissed here rather than when the button is pressed, so the dialog
      // stays up in its "working" state for as long as the request is really
      // running — deleting a voice is a round trip to DashScope, not instant.
      setPendingId(null);
    }
  }

  return (
    <Card
      description={t("settings.voices.hint")}
      action={
        <button
          onClick={refresh}
          disabled={loading || !status?.configured}
          className={`${btn.quiet} gap-1.5 px-2 py-1 text-xs`}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
          {loading
            ? t("settings.voices.refreshing")
            : t("settings.voices.refresh")}
        </button>
      }
    >
      {status !== null && !status.configured && (
        <p className="text-sm text-neutral-500">
          {t("settings.voices.needApiKey")}
        </p>
      )}
      {status?.configured && loading && voices.length === 0 && (
        <p className="text-sm text-neutral-500">{t("common.loading")}</p>
      )}
      {status?.configured && !loading && voices.length === 0 && !error && (
        <p className="text-sm text-neutral-500">{t("settings.voices.empty")}</p>
      )}
      {error && (
        <p role="alert" className="text-sm text-red-400">
          {error}
        </p>
      )}

      {voices.length > 0 && (
        <div className="space-y-2">
          {voices.map((v) => (
            <div
              key={v.voice_id}
              className="flex items-center justify-between gap-3 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2 transition duration-200 hover:border-neutral-700"
            >
              <div className="min-w-0">
                <p className="truncate font-mono text-xs text-neutral-300">
                  {v.voice_id}
                </p>
                <p className="mt-1 flex items-center gap-2 text-xs text-neutral-500">
                  <span>
                    {v.created_at
                      ? formatDateTime(v.created_at, lang)
                      : t("settings.voices.createdUnknown")}
                  </span>
                  {v.bound_character_name && (
                    <span className="rounded-full bg-emerald-500/20 px-2 py-0.5 text-emerald-400">
                      {t("settings.voices.bound", {
                        name: v.bound_character_name,
                      })}
                    </span>
                  )}
                </p>
              </div>
              <button
                onClick={() => setPendingId(v.voice_id)}
                disabled={!!v.bound_character_id || deletingId === v.voice_id}
                title={
                  v.bound_character_id
                    ? t("settings.voices.boundTitle", {
                        name: v.bound_character_name ?? "",
                      })
                    : undefined
                }
                className={`${btn.dangerOutline} shrink-0 gap-1.5 px-2.5 py-1 text-xs`}
              >
                <Trash2 className="size-3.5" />
                {deletingId === v.voice_id
                  ? t("common.deleting")
                  : t("common.delete")}
              </button>
            </div>
          ))}
        </div>
      )}

      {pendingId && (
        <ConfirmDialog
          title={t("settings.voices.deleteTitle")}
          body={t("settings.voices.deleteBody", { id: pendingId })}
          busy={deletingId === pendingId}
          onConfirm={() => handleDelete(pendingId)}
          onCancel={() => setPendingId(null)}
        />
      )}
    </Card>
  );
}

// ---- Backup ----------------------------------------------------------------

function BackupSection({
  status,
  refreshStatus,
}: {
  status: SecretStatus | null;
  refreshStatus: () => Promise<void>;
}) {
  const { lang, setLang, t } = useI18n();
  const [busy, setBusy] = useState<"export" | "import" | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmingExport, setConfirmingExport] = useState(false);
  const [confirmingRestore, setConfirmingRestore] = useState(false);
  // Default on: the reason to make a backup is to be up and running on the
  // other machine, and that needs the key. The box is only offered when
  // there is a key to include, and only matters for the user handing the
  // file to someone else.
  const [includeApiKey, setIncludeApiKey] = useState(true);
  // With no key configured there is nothing to include whatever the box
  // says, and the box isn't shown.
  const withApiKey = includeApiKey && !!status?.configured;

  async function handleExport() {
    setBusy("export");
    setError(null);
    setResult(null);
    try {
      const done = await ipc.exportBackup(withApiKey);
      // `null` is the save dialog dismissed — nothing happened, so say
      // nothing.
      if (done) {
        setResult(
          t("settings.backup.exported", { ...done.totals, path: done.path }),
        );
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      // Dismissed only once the native save dialog has been answered, so the
      // confirmation doesn't vanish and leave a system dialog on screen with
      // nothing explaining it.
      setConfirmingExport(false);
    }
  }

  /**
   * Writes back the display language the restore just stored, which is
   * what re-renders the app in it; the backend is already speaking it.
   * Never rejects — the restore's own result must not be replaced by a
   * complaint about re-reading a setting.
   */
  async function syncLanguage() {
    try {
      const stored = await ipc.getUiLanguage();
      if (isUiLanguage(stored) && stored !== lang) await setLang(stored);
    } catch (e) {
      console.error("couldn't read the display language back", e);
    }
  }

  async function handleImport() {
    setBusy("import");
    setError(null);
    setResult(null);
    try {
      const done = await ipc.importBackup();
      if (done) {
        const restored = t("settings.backup.imported", {
          ...done.totals,
          added: done.new_characters,
        });
        const settings = done.api_key_restored
          ? t("settings.backup.importedWithKey")
          : done.settings_restored
            ? t("settings.backup.importedWithSettings")
            : "";
        setResult(settings ? `${restored} ${settings}` : restored);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      // The file has just rewritten the API key, workspace, region, hotkey,
      // sensitivity and display language out from under the app. The other
      // sections read theirs afresh when opened; what shows right now is
      // the key's status and the language everything is drawn in. Read back
      // even when the import reported a failure: one that only couldn't
      // store the key still applied everything else.
      await Promise.all([refreshStatus(), syncLanguage()]);
      setBusy(null);
      // Dismissed only once the file picker it opened has been answered, so
      // the confirmation doesn't vanish and leave a native dialog on screen
      // with nothing explaining it.
      setConfirmingRestore(false);
    }
  }

  return (
    <Card description={t("settings.backup.hint")}>
      <div className="flex flex-wrap items-center gap-2">
        <button
          onClick={() => setConfirmingExport(true)}
          disabled={busy !== null}
          className={`${btn.outline} gap-2 px-3 py-2 text-sm`}
        >
          <Download className="size-4" />
          {busy === "export"
            ? t("settings.backup.exporting")
            : t("settings.backup.export")}
        </button>
        <button
          onClick={() => setConfirmingRestore(true)}
          disabled={busy !== null}
          className={`${btn.outline} gap-2 px-3 py-2 text-sm`}
        >
          <Upload className="size-4" />
          {busy === "import"
            ? t("settings.backup.importing")
            : t("settings.backup.import")}
        </button>
      </div>
      {result && <Outcome ok>{result}</Outcome>}
      {error && <Outcome ok={false}>{error}</Outcome>}

      {confirmingExport && (
        <ConfirmDialog
          title={t("settings.backup.exportTitle")}
          body={t("settings.backup.exportBody")}
          confirmLabel={t("settings.backup.exportConfirm")}
          tone="primary"
          busy={busy === "export"}
          onConfirm={handleExport}
          onCancel={() => setConfirmingExport(false)}
        >
          {status?.configured && (
            <label className="flex items-start gap-2 text-sm text-neutral-300">
              <input
                type="checkbox"
                checked={includeApiKey}
                onChange={(e) => setIncludeApiKey(e.target.checked)}
                disabled={busy === "export"}
                className="mt-0.5"
              />
              <span>
                {t("settings.backup.includeApiKey")}
                <span className="block text-xs text-neutral-500">
                  {t("settings.backup.includeApiKeyHint")}
                </span>
              </span>
            </label>
          )}
        </ConfirmDialog>
      )}

      {confirmingRestore && (
        <ConfirmDialog
          title={t("settings.backup.restoreTitle")}
          body={t("settings.backup.restoreBody")}
          confirmLabel={t("settings.backup.restoreConfirm")}
          tone="primary"
          busy={busy === "import"}
          onConfirm={handleImport}
          onCancel={() => setConfirmingRestore(false)}
        />
      )}
    </Card>
  );
}

// ---- About ----------------------------------------------------------------

function AboutSection() {
  const { lang, t } = useI18n();
  const { state, check, install } = useUpdates();
  const [version, , versionError] = useLoaded(ipc.getAppVersion);

  // Opening About is asking whether this is the newest version, so it looks
  // unless something already has.
  useEffect(() => {
    if (state.kind === "idle") void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const busy =
    state.kind === "checking" ||
    state.kind === "downloading" ||
    state.kind === "installing";
  const info =
    state.kind === "available" ||
    state.kind === "downloading" ||
    state.kind === "installing" ||
    state.kind === "failed"
      ? state.info
      : null;

  return (
    <Card
      title="VoiceChat"
      description={
        version !== undefined
          ? t("settings.about.version", { version })
          : (versionError ?? t("common.loading"))
      }
      action={
        <button
          onClick={() => void check()}
          disabled={busy}
          className={`${btn.outline} gap-2 px-3 py-2 text-sm`}
        >
          <RefreshCw
            className={`size-4 ${state.kind === "checking" ? "animate-spin" : ""}`}
          />
          {state.kind === "checking"
            ? t("settings.update.checking")
            : t("settings.update.check")}
        </button>
      }
    >
      {state.kind === "latest" && (
        <Outcome ok>{t("settings.update.latest")}</Outcome>
      )}

      {info && (
        <div className="space-y-3 rounded-lg border border-neutral-800 bg-neutral-950/60 p-4">
          <div>
            <p className="text-sm font-medium text-emerald-400">
              {t("settings.update.available", { version: info.version })}
            </p>
            {info.date && (
              <p className="mt-0.5 text-xs text-neutral-500">
                {t("settings.update.released", {
                  date: formatDateTime(info.date, lang),
                })}
              </p>
            )}
          </div>
          {info.notes && (
            <div className="max-h-48 overflow-y-auto text-sm leading-relaxed whitespace-pre-wrap text-neutral-300">
              {info.notes}
            </div>
          )}

          {state.kind === "downloading" ? (
            <DownloadProgress
              downloaded={state.downloaded}
              total={state.total}
            />
          ) : state.kind === "installing" ? (
            <p className="flex items-center gap-1.5 text-sm text-neutral-400">
              <LoaderCircle className="size-4 animate-spin" />
              {t("settings.update.installing")}
            </p>
          ) : (
            <div className="space-y-2">
              <button
                onClick={() => void install()}
                className={`${btn.primary} gap-2 px-3 py-2 text-sm`}
              >
                <Download className="size-4" />
                {t("settings.update.install")}
              </button>
              <p className="text-xs leading-relaxed text-neutral-500">
                {t("settings.update.installHint")}
              </p>
            </div>
          )}
        </div>
      )}

      {state.kind === "failed" && <Outcome ok={false}>{state.message}</Outcome>}
    </Card>
  );
}

function DownloadProgress({
  downloaded,
  total,
}: {
  downloaded: number;
  total: number | null;
}) {
  const t = useT();
  const percent = total
    ? Math.min(100, Math.floor((downloaded / total) * 100))
    : null;
  return (
    <div className="space-y-1.5">
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-neutral-800">
        <div
          className={`h-full bg-emerald-400 transition-[width] duration-200 ${percent === null ? "w-1/3 animate-pulse" : ""}`}
          style={percent === null ? undefined : { width: `${percent}%` }}
        />
      </div>
      <p aria-live="polite" className="text-xs text-neutral-500">
        {percent !== null
          ? t("settings.update.downloading", { percent })
          : t("settings.update.downloadingSize", {
              size: (downloaded / 1024 / 1024).toFixed(1),
            })}
      </p>
    </div>
  );
}
