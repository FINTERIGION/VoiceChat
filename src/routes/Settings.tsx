import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import {
  isUiLanguage,
  UI_LANGUAGES,
  useI18n,
  type UiLanguage,
} from "../lib/i18n";
import { ipc } from "../lib/ipc";
import type { ManagedVoice, RegionOption, SecretStatus } from "../lib/types";
import { btn } from "../lib/ui";

type TestState =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "success" }
  | { kind: "error"; message: string };

/**
 * Saving connection settings can fail validation now — a workspace id that
 * isn't one — and that reason has to reach the user, so unlike the other
 * save indicators on this page this one carries the message rather than
 * collapsing to a generic failure.
 */
type SaveState =
  | { kind: "idle" }
  | { kind: "saved" }
  | { kind: "error"; message: string };

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

export default function Settings() {
  const { lang, setLang, t } = useI18n();
  const [langError, setLangError] = useState(false);
  const [status, setStatus] = useState<SecretStatus | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [workspaceId, setWorkspaceId] = useState("");
  const [region, setRegion] = useState("");
  const [regions, setRegions] = useState<RegionOption[]>([]);
  const [saveState, setSaveState] = useState<SaveState>({ kind: "idle" });
  const [testState, setTestState] = useState<TestState>({ kind: "idle" });
  const [vadThreshold, setVadThreshold] = useState(0.5);
  const [vadSilenceMs, setVadSilenceMs] = useState(800);
  const [vadSaveState, setVadSaveState] = useState<"idle" | "saved" | "error">(
    "idle",
  );
  const [hotkey, setHotkey] = useState<string | null>(null);
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [hotkeySaveState, setHotkeySaveState] = useState<
    "idle" | "saved" | "error"
  >("idle");
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [voices, setVoices] = useState<ManagedVoice[]>([]);
  const [voicesLoading, setVoicesLoading] = useState(false);
  const [voicesError, setVoicesError] = useState<string | null>(null);
  const [deletingVoiceId, setDeletingVoiceId] = useState<string | null>(null);
  const [pendingVoiceId, setPendingVoiceId] = useState<string | null>(null);
  const [backupBusy, setBackupBusy] = useState<"export" | "import" | null>(
    null,
  );
  const [backupResult, setBackupResult] = useState<string | null>(null);
  const [backupError, setBackupError] = useState<string | null>(null);
  const [confirmingRestore, setConfirmingRestore] = useState(false);
  const [confirmingExport, setConfirmingExport] = useState(false);
  // Default on: the reason to make a backup is to be up and running on the
  // other machine, and that needs the key. The box is only offered when
  // there is a key to include, and only matters for the user handing the
  // file to someone else.
  const [includeApiKey, setIncludeApiKey] = useState(true);
  // With no key configured there is nothing to include whatever the box
  // says, and the box isn't shown.
  const withApiKey = includeApiKey && !!status?.configured;

  async function refreshStatus() {
    setStatus(await ipc.getSecretStatus());
  }

  /**
   * Every field on this page, read back from the backend. Called on mount,
   * and again after a restore — which can have replaced all of them at once,
   * and would otherwise leave the page showing the settings of the device
   * the backup came off.
   *
   * Never rejects: these are local reads, and the one caller that has
   * something to say — the restore — must not have its own result replaced
   * by a complaint about re-reading a text box.
   */
  async function loadSettings() {
    try {
      const [secret, connection, vad, storedHotkey, storedLang] =
        await Promise.all([
          ipc.getSecretStatus(),
          ipc.getConnectionSettings(),
          ipc.getVadSettings(),
          ipc.getHotkey(),
          ipc.getUiLanguage(),
        ]);
      setStatus(secret);
      setWorkspaceId(connection.workspace_id ?? "");
      setRegion(connection.region ?? "");
      setVadThreshold(vad.threshold);
      setVadSilenceMs(vad.silence_ms);
      setHotkey(storedHotkey);
      // Writes back the value it just read, which is what re-renders the app
      // in the restored language; the backend is already speaking it.
      if (isUiLanguage(storedLang) && storedLang !== lang) {
        await setLang(storedLang);
      }
    } catch (e) {
      console.error("couldn't read the settings back", e);
    }
  }

  useEffect(() => {
    loadSettings();
  }, []);

  // Region labels are produced by the backend in the current display
  // language, so they have to be re-fetched whenever that changes.
  useEffect(() => {
    ipc.listRegions().then(setRegions);
  }, [lang]);

  async function handleLanguageChange(next: UiLanguage) {
    setLangError(false);
    try {
      await setLang(next);
    } catch {
      setLangError(true);
    }
  }

  async function refreshVoices() {
    setVoicesLoading(true);
    setVoicesError(null);
    try {
      setVoices(await ipc.listVoices());
    } catch (e) {
      setVoicesError(String(e));
    } finally {
      setVoicesLoading(false);
    }
  }

  useEffect(() => {
    if (status?.configured) {
      refreshVoices();
    }
  }, [status?.configured]);

  async function handleDeleteVoice(voiceId: string) {
    setDeletingVoiceId(voiceId);
    setVoicesError(null);
    try {
      await ipc.deleteVoice(voiceId);
      await refreshVoices();
    } catch (e) {
      setVoicesError(String(e));
    } finally {
      setDeletingVoiceId(null);
      // Dismissed here rather than when the button is pressed, so the dialog
      // stays up in its "working" state for as long as the request is really
      // running — deleting a voice is a round trip to DashScope, not instant.
      setPendingVoiceId(null);
    }
  }

  async function handleExportBackup() {
    setBackupBusy("export");
    setBackupError(null);
    setBackupResult(null);
    try {
      const done = await ipc.exportBackup(withApiKey);
      // `null` is the save dialog dismissed — nothing happened, so say
      // nothing.
      if (done) {
        setBackupResult(
          t("settings.backup.exported", { ...done.totals, path: done.path }),
        );
      }
    } catch (e) {
      setBackupError(String(e));
    } finally {
      setBackupBusy(null);
      // Dismissed only once the native save dialog has been answered, so the
      // confirmation doesn't vanish and leave a system dialog on screen with
      // nothing explaining it.
      setConfirmingExport(false);
    }
  }

  async function handleImportBackup() {
    setBackupBusy("import");
    setBackupError(null);
    setBackupResult(null);
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
        setBackupResult(settings ? `${restored} ${settings}` : restored);
      }
    } catch (e) {
      setBackupError(String(e));
    } finally {
      // The file has just rewritten the API key, workspace, region, hotkey,
      // sensitivity and display language out from under this page. Read back
      // even when the import reported a failure: one that only couldn't
      // store the key still applied everything else.
      await loadSettings();
      setBackupBusy(null);
      // Dismissed only once the file picker it opened has been answered, so
      // the confirmation doesn't vanish and leave a native dialog on screen
      // with nothing explaining it.
      setConfirmingRestore(false);
    }
  }

  async function handleSaveVadSettings() {
    try {
      await ipc.setVadSettings({ threshold: vadThreshold, silence_ms: vadSilenceMs });
      setVadSaveState("saved");
      setTimeout(() => setVadSaveState("idle"), 1500);
    } catch {
      setVadSaveState("error");
    }
  }

  useEffect(() => {
    if (!capturingHotkey) return;
    function onKeyDown(e: KeyboardEvent) {
      e.preventDefault();
      // A bare modifier press isn't a complete combo yet — keep waiting.
      if (MODIFIER_CODES.has(e.code)) return;
      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      if (e.metaKey) parts.push("Super");
      parts.push(e.code);
      setHotkey(parts.join("+"));
      setCapturingHotkey(false);
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [capturingHotkey]);

  async function handleSaveHotkey() {
    setHotkeyError(null);
    try {
      await ipc.setHotkey(hotkey);
      setHotkeySaveState("saved");
      setTimeout(() => setHotkeySaveState("idle"), 1500);
    } catch (e) {
      setHotkeySaveState("error");
      setHotkeyError(String(e));
    }
  }

  async function handleSaveApiKey() {
    if (!apiKeyInput.trim()) return;
    try {
      await ipc.setApiKey(apiKeyInput.trim());
      setApiKeyInput("");
      setTestState({ kind: "idle" });
      await refreshStatus();
    } catch (e) {
      setTestState({ kind: "error", message: String(e) });
    }
  }

  async function handleClearApiKey() {
    await ipc.clearApiKey();
    setTestState({ kind: "idle" });
    await refreshStatus();
  }

  async function handleSaveConnectionSettings() {
    try {
      await ipc.setConnectionSettings({
        workspace_id: workspaceId.trim() || null,
        region: region.trim() || null,
      });
      setSaveState({ kind: "saved" });
      setTimeout(() => setSaveState({ kind: "idle" }), 1500);
    } catch (e) {
      setSaveState({ kind: "error", message: String(e) });
    }
  }

  async function handleTestConnectivity() {
    setTestState({ kind: "pending" });
    try {
      await ipc.testConnectivity();
      setTestState({ kind: "success" });
    } catch (e) {
      setTestState({ kind: "error", message: String(e) });
    }
  }

  return (
    <div className="mx-auto max-w-xl space-y-8 p-8 text-neutral-100">
      <h1 className="text-xl font-semibold">{t("settings.title")}</h1>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.language.heading")}
        </h2>
        <select
          value={lang}
          onChange={(e) => handleLanguageChange(e.target.value as UiLanguage)}
          className="w-full max-w-xs rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        >
          {UI_LANGUAGES.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
        <p className="text-xs text-neutral-500">{t("settings.language.hint")}</p>
        {langError && (
          <p className="text-xs text-red-400">{t("common.saveFailed")}</p>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.apiKey.heading")}
        </h2>

        <p className="text-sm text-neutral-300">
          {status === null
            ? t("common.loading")
            : status.configured
              ? t("settings.apiKey.configured", { tail: status.tail ?? "" })
              : t("settings.apiKey.missing")}
        </p>

        <div className="flex gap-2">
          <input
            type="password"
            value={apiKeyInput}
            onChange={(e) => setApiKeyInput(e.target.value)}
            placeholder="sk-..."
            className="flex-1 rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
          />
          <button
            onClick={handleSaveApiKey}
            disabled={!apiKeyInput.trim()}
            className={`${btn.primary} px-3 py-2 text-sm font-medium`}
          >
            {t("common.save")}
          </button>
          {status?.configured && (
            <button
              onClick={handleClearApiKey}
              className={`${btn.outline} px-3 py-2 text-sm`}
            >
              {t("settings.apiKey.clear")}
            </button>
          )}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.optional.heading")}
        </h2>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              {t("settings.optional.workspaceId")}
            </label>
            <input
              value={workspaceId}
              onChange={(e) => setWorkspaceId(e.target.value)}
              placeholder={t("settings.optional.workspacePlaceholder")}
              className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
            />
          </div>
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              {t("settings.optional.region")}
            </label>
            <select
              value={region}
              onChange={(e) => setRegion(e.target.value)}
              className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
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
          </div>
        </div>
        <p className="text-xs text-neutral-500">{t("settings.optional.hint")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <button
            onClick={handleSaveConnectionSettings}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {t("settings.saveConfig")}
          </button>
          <button
            onClick={handleTestConnectivity}
            disabled={testState.kind === "pending" || !status?.configured}
            className={`${btn.primary} px-3 py-2 text-sm font-medium`}
          >
            {testState.kind === "pending"
              ? t("settings.connectivity.testing")
              : t("settings.connectivity.test")}
          </button>
          {saveState.kind === "saved" && (
            <span className="text-xs text-emerald-400">{t("common.saved")}</span>
          )}
        </div>
        {saveState.kind === "error" && (
          <p className="text-sm text-red-400">{saveState.message}</p>
        )}
        {testState.kind === "success" && (
          <p className="text-sm text-emerald-400">
            {t("settings.connectivity.ok")}
          </p>
        )}
        {testState.kind === "error" && (
          <p className="text-sm text-red-400">✗ {testState.message}</p>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.vad.heading")}
        </h2>
        <p className="text-xs text-neutral-500">{t("settings.vad.hint")}</p>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              {t("settings.vad.threshold", { value: vadThreshold.toFixed(2) })}
            </label>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={vadThreshold}
              onChange={(e) => setVadThreshold(Number(e.target.value))}
              className="w-full"
            />
          </div>
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              {t("settings.vad.silence", { ms: vadSilenceMs })}
            </label>
            <input
              type="range"
              min={200}
              max={6000}
              step={100}
              value={vadSilenceMs}
              onChange={(e) => setVadSilenceMs(Number(e.target.value))}
              className="w-full"
            />
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            onClick={handleSaveVadSettings}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {t("settings.saveConfig")}
          </button>
          {vadSaveState === "saved" && (
            <span className="text-xs text-emerald-400">{t("common.saved")}</span>
          )}
          {vadSaveState === "error" && (
            <span className="text-xs text-red-400">{t("common.saveFailed")}</span>
          )}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.hotkey.heading")}
        </h2>
        <p className="text-xs text-neutral-500">{t("settings.hotkey.hint")}</p>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setCapturingHotkey(true)}
            className={`${btn.outline} min-w-[10rem] bg-neutral-900 px-3 py-2 text-sm`}
          >
            {capturingHotkey
              ? t("settings.hotkey.capturing")
              : (hotkey ?? t("common.notSet"))}
          </button>
          <button
            onClick={() => setHotkey(null)}
            disabled={hotkey === null}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {t("settings.hotkey.disable")}
          </button>
          <button
            onClick={handleSaveHotkey}
            className={`${btn.primary} px-3 py-2 text-sm font-medium`}
          >
            {t("common.save")}
          </button>
        </div>
        {hotkeySaveState === "saved" && (
          <span className="text-xs text-emerald-400">{t("common.saved")}</span>
        )}
        {hotkeySaveState === "error" && (
          <p className="text-sm text-red-400">{hotkeyError}</p>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-neutral-400">
            {t("settings.voices.heading")}
          </h2>
          <button
            onClick={refreshVoices}
            disabled={voicesLoading || !status?.configured}
            className={`${btn.quiet} px-2 py-1 text-xs`}
          >
            {voicesLoading
              ? t("settings.voices.refreshing")
              : t("settings.voices.refresh")}
          </button>
        </div>
        <p className="text-xs text-neutral-500">{t("settings.voices.hint")}</p>

        {!status?.configured && (
          <p className="text-sm text-neutral-500">
            {t("settings.voices.needApiKey")}
          </p>
        )}
        {status?.configured && voicesLoading && voices.length === 0 && (
          <p className="text-sm text-neutral-500">{t("common.loading")}</p>
        )}
        {status?.configured &&
          !voicesLoading &&
          voices.length === 0 &&
          !voicesError && (
            <p className="text-sm text-neutral-500">
              {t("settings.voices.empty")}
            </p>
          )}
        {voicesError && <p className="text-sm text-red-400">{voicesError}</p>}

        <div className="space-y-2">
          {voices.map((v) => (
            <div
              key={v.voice_id}
              className="flex items-center justify-between gap-3 rounded-lg border border-neutral-800 px-3 py-2 transition duration-200 hover:border-neutral-700 hover:bg-neutral-900/60"
            >
              <div className="min-w-0">
                <p className="truncate font-mono text-xs text-neutral-300">
                  {v.voice_id}
                </p>
                <p className="mt-1 flex items-center gap-2 text-xs text-neutral-500">
                  <span>
                    {v.created_at ?? t("settings.voices.createdUnknown")}
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
                onClick={() => setPendingVoiceId(v.voice_id)}
                disabled={!!v.bound_character_id || deletingVoiceId === v.voice_id}
                title={
                  v.bound_character_id
                    ? t("settings.voices.boundTitle", {
                        name: v.bound_character_name ?? "",
                      })
                    : undefined
                }
                className={`${btn.dangerOutline} shrink-0 px-2.5 py-1 text-xs`}
              >
                {deletingVoiceId === v.voice_id
                  ? t("common.deleting")
                  : t("common.delete")}
              </button>
            </div>
          ))}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          {t("settings.backup.heading")}
        </h2>
        <p className="text-xs text-neutral-500">{t("settings.backup.hint")}</p>
        <div className="flex flex-wrap items-center gap-2">
          <button
            onClick={() => setConfirmingExport(true)}
            disabled={backupBusy !== null}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {backupBusy === "export"
              ? t("settings.backup.exporting")
              : t("settings.backup.export")}
          </button>
          <button
            onClick={() => setConfirmingRestore(true)}
            disabled={backupBusy !== null}
            className={`${btn.outline} px-3 py-2 text-sm`}
          >
            {backupBusy === "import"
              ? t("settings.backup.importing")
              : t("settings.backup.import")}
          </button>
        </div>
        {backupResult && (
          <p className="text-sm break-all text-emerald-400">{backupResult}</p>
        )}
        {backupError && <p className="text-sm text-red-400">{backupError}</p>}
      </section>

      {confirmingExport && (
        <ConfirmDialog
          title={t("settings.backup.exportTitle")}
          body={t("settings.backup.exportBody")}
          confirmLabel={t("settings.backup.exportConfirm")}
          tone="primary"
          busy={backupBusy === "export"}
          onConfirm={handleExportBackup}
          onCancel={() => setConfirmingExport(false)}
        >
          {status?.configured && (
            <label className="flex items-start gap-2 text-sm text-neutral-300">
              <input
                type="checkbox"
                checked={includeApiKey}
                onChange={(e) => setIncludeApiKey(e.target.checked)}
                disabled={backupBusy === "export"}
                className="mt-0.5 accent-emerald-500"
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
          busy={backupBusy === "import"}
          onConfirm={handleImportBackup}
          onCancel={() => setConfirmingRestore(false)}
        />
      )}

      {pendingVoiceId && (
        <ConfirmDialog
          title={t("settings.voices.deleteTitle")}
          body={t("settings.voices.deleteBody", { id: pendingVoiceId })}
          busy={deletingVoiceId === pendingVoiceId}
          onConfirm={() => handleDeleteVoice(pendingVoiceId)}
          onCancel={() => setPendingVoiceId(null)}
        />
      )}
    </div>
  );
}
