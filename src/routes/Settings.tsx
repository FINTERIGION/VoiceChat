import { useEffect, useState } from "react";
import ConfirmDialog from "../components/ConfirmDialog";
import { ipc } from "../lib/ipc";
import type { ManagedVoice, RegionOption, SecretStatus } from "../lib/types";

type TestState =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "success" }
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
  const [status, setStatus] = useState<SecretStatus | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [workspaceId, setWorkspaceId] = useState("");
  const [region, setRegion] = useState("");
  const [regions, setRegions] = useState<RegionOption[]>([]);
  const [saveState, setSaveState] = useState<"idle" | "saved" | "error">(
    "idle",
  );
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

  async function refreshStatus() {
    setStatus(await ipc.getSecretStatus());
  }

  useEffect(() => {
    refreshStatus();
    ipc.getConnectionSettings().then((s) => {
      setWorkspaceId(s.workspace_id ?? "");
      setRegion(s.region ?? "");
    });
    ipc.listRegions().then(setRegions);
    ipc.getVadSettings().then((s) => {
      setVadThreshold(s.threshold);
      setVadSilenceMs(s.silence_ms);
    });
    ipc.getHotkey().then(setHotkey);
  }, []);

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
      // stays up in its "处理中…" state for as long as the request is really
      // running — deleting a voice is a round trip to DashScope, not instant.
      setPendingVoiceId(null);
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
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 1500);
    } catch {
      setSaveState("error");
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
      <h1 className="text-xl font-semibold">设置</h1>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">
          DashScope API Key
        </h2>

        <p className="text-sm text-neutral-300">
          {status === null
            ? "加载中…"
            : status.configured
              ? `已配置（${status.tail}）`
              : "尚未配置"}
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
            className="rounded bg-neutral-100 px-3 py-2 text-sm font-medium text-neutral-900 disabled:opacity-40"
          >
            保存
          </button>
          {status?.configured && (
            <button
              onClick={handleClearApiKey}
              className="rounded border border-neutral-700 px-3 py-2 text-sm text-neutral-300"
            >
              清除
            </button>
          )}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">可选配置</h2>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              WorkspaceId
            </label>
            <input
              value={workspaceId}
              onChange={(e) => setWorkspaceId(e.target.value)}
              placeholder="留空使用旧域名"
              className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
            />
          </div>
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              地域
            </label>
            <select
              value={region}
              onChange={(e) => setRegion(e.target.value)}
              className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-500"
            >
              <option value="">
                默认（{regions.find((r) => r.id === "cn-beijing")?.label ?? "北京"}）
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
        <p className="text-xs text-neutral-500">
          语音克隆/设计和实时对话目前仅在这两个地域可用，选择离你更近的地域可以降低延迟。切换后需要重新测试连通性。
        </p>
        <div className="flex items-center gap-3">
          <button
            onClick={handleSaveConnectionSettings}
            className="rounded border border-neutral-700 px-3 py-2 text-sm text-neutral-300"
          >
            保存配置
          </button>
          {saveState === "saved" && (
            <span className="text-xs text-emerald-400">已保存</span>
          )}
          {saveState === "error" && (
            <span className="text-xs text-red-400">保存失败</span>
          )}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">免提检测</h2>
        <p className="text-xs text-neutral-500">
          控制麦克风开着的时候，多灵敏能听出你在说话、停顿多久算你说完了。
        </p>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1 block text-xs text-neutral-500">
              VAD 阈值 ({vadThreshold.toFixed(2)})
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
              静音判定 ({vadSilenceMs}ms)
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
            className="rounded border border-neutral-700 px-3 py-2 text-sm text-neutral-300"
          >
            保存配置
          </button>
          {vadSaveState === "saved" && (
            <span className="text-xs text-emerald-400">已保存</span>
          )}
          {vadSaveState === "error" && (
            <span className="text-xs text-red-400">保存失败</span>
          )}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">全局快捷键</h2>
        <p className="text-xs text-neutral-500">
          在任意窗口按一下即可开关麦克风。点击下面的按钮，然后按下想要的按键组合。
        </p>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setCapturingHotkey(true)}
            className="min-w-[10rem] rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-left text-sm text-neutral-200"
          >
            {capturingHotkey ? "请按下新快捷键…" : (hotkey ?? "未设置")}
          </button>
          <button
            onClick={() => setHotkey(null)}
            disabled={hotkey === null}
            className="rounded border border-neutral-700 px-3 py-2 text-sm text-neutral-300 disabled:opacity-40"
          >
            禁用
          </button>
          <button
            onClick={handleSaveHotkey}
            className="rounded bg-neutral-100 px-3 py-2 text-sm font-medium text-neutral-900"
          >
            保存
          </button>
        </div>
        {hotkeySaveState === "saved" && (
          <span className="text-xs text-emerald-400">已保存</span>
        )}
        {hotkeySaveState === "error" && (
          <p className="text-sm text-red-400">{hotkeyError}</p>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-medium text-neutral-400">连通性测试</h2>
        <button
          onClick={handleTestConnectivity}
          disabled={testState.kind === "pending" || !status?.configured}
          className="rounded bg-neutral-100 px-3 py-2 text-sm font-medium text-neutral-900 disabled:opacity-40"
        >
          {testState.kind === "pending" ? "测试中…" : "测试连通性"}
        </button>
        {testState.kind === "success" && (
          <p className="text-sm text-emerald-400">✓ 连接成功</p>
        )}
        {testState.kind === "error" && (
          <p className="text-sm text-red-400">✗ {testState.message}</p>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-neutral-400">音色管理</h2>
          <button
            onClick={refreshVoices}
            disabled={voicesLoading || !status?.configured}
            className="text-xs text-neutral-500 hover:text-neutral-300 disabled:opacity-40"
          >
            {voicesLoading ? "刷新中…" : "刷新"}
          </button>
        </div>
        <p className="text-xs text-neutral-500">
          通过「录音复刻」「文本设计」创建的自定义音色，保存在你的 DashScope
          账号下。已被角色绑定的音色不能在这里删除，需要先去该角色换一个音色。
        </p>

        {!status?.configured && (
          <p className="text-sm text-neutral-500">请先配置 API Key。</p>
        )}
        {status?.configured && voicesLoading && voices.length === 0 && (
          <p className="text-sm text-neutral-500">加载中…</p>
        )}
        {status?.configured &&
          !voicesLoading &&
          voices.length === 0 &&
          !voicesError && (
            <p className="text-sm text-neutral-500">还没有自定义音色。</p>
          )}
        {voicesError && <p className="text-sm text-red-400">{voicesError}</p>}

        <div className="space-y-2">
          {voices.map((v) => (
            <div
              key={v.voice_id}
              className="flex items-center justify-between gap-3 rounded-lg border border-neutral-800 px-3 py-2"
            >
              <div className="min-w-0">
                <p className="truncate font-mono text-xs text-neutral-300">
                  {v.voice_id}
                </p>
                <p className="mt-1 flex items-center gap-2 text-xs text-neutral-500">
                  <span>{v.created_at ?? "创建时间未知"}</span>
                  {v.bound_character_name && (
                    <span className="rounded-full bg-emerald-500/20 px-2 py-0.5 text-emerald-400">
                      绑定：{v.bound_character_name}
                    </span>
                  )}
                </p>
              </div>
              <button
                onClick={() => setPendingVoiceId(v.voice_id)}
                disabled={!!v.bound_character_id || deletingVoiceId === v.voice_id}
                title={
                  v.bound_character_id
                    ? `已绑定角色「${v.bound_character_name}」，无法删除`
                    : undefined
                }
                className="shrink-0 rounded border border-neutral-800 px-2.5 py-1 text-xs text-red-400 disabled:opacity-30"
              >
                {deletingVoiceId === v.voice_id ? "删除中…" : "删除"}
              </button>
            </div>
          ))}
        </div>
      </section>

      {pendingVoiceId && (
        <ConfirmDialog
          title="删除音色？"
          body={`${pendingVoiceId} 会从你的 DashScope 账号中永久删除，无法恢复；如果之后还想用，需要重新录制或重新设计。`}
          busy={deletingVoiceId === pendingVoiceId}
          onConfirm={() => handleDeleteVoice(pendingVoiceId)}
          onCancel={() => setPendingVoiceId(null)}
        />
      )}
    </div>
  );
}
