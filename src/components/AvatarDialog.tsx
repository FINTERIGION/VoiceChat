import {
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useRef,
  useState,
  type Ref,
} from "react";
import {
  ArrowLeft,
  ImageUp,
  LoaderCircle,
  RefreshCw,
  Sparkles,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { useT, type MessageKey } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import { btn, btnBase, field, tabBtn } from "../lib/ui";
import Modal from "./Modal";

/** On-screen size of the crop viewport, in CSS pixels. */
const VIEW = 256;
/** Stored avatars are this many pixels square. */
const OUT = 512;
const MAX_ZOOM = 4;
/** Bigger than any photo worth cropping to 512 px, and small enough to read into memory. */
const MAX_UPLOAD_BYTES = 20 * 1024 * 1024;

type Tab = "upload" | "generate";
type Style = "anime" | "realistic" | "cartoon3d" | "watercolor" | "flat";

const STYLES: readonly Style[] = [
  "anime",
  "realistic",
  "cartoon3d",
  "watercolor",
  "flat",
];

const STYLE_LABEL: Record<Style, MessageKey> = {
  anime: "avatar.style.anime",
  realistic: "avatar.style.realistic",
  cartoon3d: "avatar.style.cartoon3d",
  watercolor: "avatar.style.watercolor",
  flat: "avatar.style.flat",
};

const STYLE_PROMPT: Record<Style, MessageKey> = {
  anime: "avatar.stylePrompt.anime",
  realistic: "avatar.stylePrompt.realistic",
  cartoon3d: "avatar.stylePrompt.cartoon3d",
  watercolor: "avatar.stylePrompt.watercolor",
  flat: "avatar.stylePrompt.flat",
};

/**
 * What the description box starts out holding: the character as the user
 * already wrote them. Clipped, because a persona runs to paragraphs about
 * temperament that say nothing about a face.
 */
function defaultDescription(name: string, persona: string): string {
  const who = name.trim();
  const about = persona.trim().slice(0, 160);
  return [who, about].filter(Boolean).join("，");
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/**
 * Picks a new picture for a character — one of the user's own, or one drawn
 * by Qwen-Image from a description — and crops it to a circle.
 *
 * `onSaved` receives the stored picture's name once it is on disk; the
 * dialog stays up, busy, until it settles, so a caller that applies the
 * picture straight away (the Characters tab) can report a failure here.
 */
export default function AvatarDialog({
  characterName,
  persona,
  onSaved,
  onClose,
}: {
  characterName: string;
  persona: string;
  onSaved: (avatarPath: string) => Promise<void> | void;
  onClose: () => void;
}) {
  const t = useT();
  const titleId = useId();
  const fileInput = useRef<HTMLInputElement>(null);
  const cropper = useRef<CropperHandle>(null);
  const [tab, setTab] = useState<Tab>("upload");
  // The picture being cropped, and where it came from — a generated one
  // can be drawn again from the crop step.
  const [source, setSource] = useState<{ src: string; from: Tab } | null>(null);
  const [description, setDescription] = useState(() =>
    defaultDescription(characterName, persona),
  );
  const [style, setStyle] = useState<Style>("anime");
  const [generating, setGenerating] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A generation outlives a dialog closed while it runs; its answer must not
  // land in a component that is gone, or in a newer attempt's place.
  const attempt = useRef(0);
  useEffect(() => () => void attempt.current++, []);

  useEffect(() => {
    if (!generating) return;
    setElapsed(0);
    const started = Date.now();
    const timer = window.setInterval(
      () => setElapsed(Math.floor((Date.now() - started) / 1000)),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [generating]);

  async function handleFile(file: File | undefined) {
    if (!file) return;
    setError(null);
    if (!file.type.startsWith("image/")) {
      setError(t("avatar.notImage"));
      return;
    }
    if (file.size > MAX_UPLOAD_BYTES) {
      setError(t("avatar.tooLarge", { mb: MAX_UPLOAD_BYTES / 1_048_576 }));
      return;
    }
    try {
      setSource({ src: await readAsDataUrl(file), from: "upload" });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleGenerate() {
    const text = description.trim();
    if (!text || generating) return;
    const mine = ++attempt.current;
    setGenerating(true);
    setError(null);
    try {
      const prompt = [
        t(STYLE_PROMPT[style]),
        text,
        t("avatar.promptSuffix"),
      ].join("。");
      const src = await ipc.generateAvatar(prompt);
      if (mine !== attempt.current) return;
      setSource({ src, from: "generate" });
    } catch (e) {
      if (mine === attempt.current) setError(String(e));
    } finally {
      if (mine === attempt.current) setGenerating(false);
    }
  }

  async function handleUse() {
    const dataUrl = cropper.current?.exportCrop();
    if (!dataUrl) return;
    setSaving(true);
    setError(null);
    try {
      const name = await ipc.saveAvatar(dataUrl);
      await onSaved(name);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  function backToChoose() {
    setError(null);
    if (source) setTab(source.from);
    setSource(null);
  }

  const handleCropError = useCallback(() => {
    setError(t("avatar.unreadable"));
    setSource(null);
  }, [t]);

  return (
    <Modal
      labelledBy={titleId}
      onClose={saving ? undefined : onClose}
      className="flex max-h-full w-full max-w-md flex-col rounded-2xl border border-neutral-800 bg-neutral-950 text-neutral-100"
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
        <h2 id={titleId} className="text-sm font-medium">
          {t("avatar.title")}
        </h2>
        <button
          onClick={onClose}
          disabled={saving}
          title={t("common.close")}
          aria-label={t("common.close")}
          className={`${btn.quiet} size-7 hover:rotate-90`}
        >
          <X className="size-4" />
        </button>
      </div>

      {source ? (
        <>
          <div className="relative flex flex-col items-center gap-3 overflow-y-auto p-5">
            <Cropper ref={cropper} src={source.src} onError={handleCropError} />
            <p className="text-xs text-neutral-500">{t("avatar.cropHint")}</p>
            {generating && <GeneratingOverlay elapsed={elapsed} />}
          </div>
          {error && <ErrorLine message={error} />}
          <div className="flex items-center gap-2 border-t border-neutral-800 px-5 py-3">
            <button
              onClick={backToChoose}
              disabled={saving || generating}
              className={`${btn.quiet} gap-1 px-1.5 py-1.5 text-sm`}
            >
              <ArrowLeft className="size-4" />
              {t("common.back")}
            </button>
            {source.from === "generate" && (
              <button
                onClick={handleGenerate}
                disabled={saving || generating}
                className={`${btn.outline} gap-1.5 px-3 py-1.5 text-sm`}
              >
                <RefreshCw className="size-3.5" />
                {t("avatar.regenerate")}
              </button>
            )}
            <button
              onClick={handleUse}
              disabled={saving || generating}
              className={`${btn.primary} ml-auto px-4 py-1.5 text-sm font-medium`}
            >
              {saving ? t("common.saving") : t("avatar.use")}
            </button>
          </div>
        </>
      ) : (
        <>
          <nav className="flex gap-1 border-b border-neutral-800 px-5 pt-2">
            {(
              [
                ["upload", "avatar.tab.upload"],
                ["generate", "avatar.tab.generate"],
              ] as const satisfies readonly (readonly [Tab, MessageKey])[]
            ).map(([key, label]) => (
              <button
                key={key}
                onClick={() => {
                  setTab(key);
                  setError(null);
                }}
                disabled={generating}
                className={tabBtn(tab === key)}
              >
                {t(label)}
              </button>
            ))}
          </nav>

          <div className="relative overflow-y-auto p-5">
            {tab === "upload" ? (
              <>
                <button
                  onClick={() => fileInput.current?.click()}
                  className={`${btnBase} w-full flex-col gap-2 rounded-xl border border-dashed border-neutral-700 py-10 text-neutral-400 not-disabled:hover:border-neutral-500 not-disabled:hover:bg-neutral-900 not-disabled:hover:text-neutral-200`}
                >
                  <ImageUp className="size-6" />
                  <span className="text-sm">{t("avatar.choose")}</span>
                  <span className="text-xs text-neutral-500">
                    {t("avatar.chooseHint")}
                  </span>
                </button>
                <input
                  ref={fileInput}
                  type="file"
                  accept="image/*"
                  className="hidden"
                  onChange={(e) => {
                    void handleFile(e.target.files?.[0]);
                    // So picking the same file again still fires `change`.
                    e.target.value = "";
                  }}
                />
              </>
            ) : (
              <div className="space-y-4">
                <label className="block">
                  <span className="mb-1.5 block text-xs text-neutral-400">
                    {t("avatar.describe")}
                  </span>
                  <textarea
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    rows={4}
                    placeholder={t("avatar.describePlaceholder")}
                    className={`${field} w-full resize-y`}
                  />
                </label>
                <div>
                  <p className="mb-1.5 text-xs text-neutral-400">
                    {t("avatar.style")}
                  </p>
                  <div className="flex flex-wrap gap-1.5" role="radiogroup">
                    {STYLES.map((s) => (
                      <button
                        key={s}
                        role="radio"
                        aria-checked={style === s}
                        onClick={() => setStyle(s)}
                        className={`${btnBase} rounded-full border px-3 py-1 text-xs ${
                          style === s
                            ? "border-emerald-500/60 bg-emerald-500/15 text-emerald-300"
                            : "border-neutral-700 text-neutral-400 not-disabled:hover:border-neutral-500 not-disabled:hover:text-neutral-200"
                        }`}
                      >
                        {t(STYLE_LABEL[s])}
                      </button>
                    ))}
                  </div>
                </div>
                <button
                  onClick={handleGenerate}
                  disabled={generating || !description.trim()}
                  className={`${btn.primary} w-full gap-1.5 py-2 text-sm font-medium`}
                >
                  <Sparkles className="size-4" />
                  {t("avatar.generate")}
                </button>
                <p className="text-xs leading-relaxed text-neutral-500">
                  {t("avatar.generateHint")}
                </p>
                {generating && <GeneratingOverlay elapsed={elapsed} />}
              </div>
            )}
          </div>
          {error && <ErrorLine message={error} />}
        </>
      )}
    </Modal>
  );
}

function ErrorLine({ message }: { message: string }) {
  return (
    <p
      role="alert"
      className="mx-5 mb-3 rounded-lg bg-red-500/10 px-3 py-2 text-xs leading-relaxed break-words text-red-400"
    >
      {message}
    </p>
  );
}

function GeneratingOverlay({ elapsed }: { elapsed: number }) {
  const t = useT();
  return (
    <div
      role="status"
      className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-neutral-950/85 text-sm text-neutral-300"
    >
      <LoaderCircle className="size-6 animate-spin text-emerald-400" />
      {t("avatar.generating", { seconds: elapsed })}
      <span className="text-xs text-neutral-500">
        {t("avatar.generatingHint")}
      </span>
    </div>
  );
}

interface CropperHandle {
  /** The visible square as a 512 px image, WebP where the webview can encode it. */
  exportCrop: () => string | null;
}

/** Zoom 1 fits the picture's short side to the viewport; x/y is its top-left corner. */
interface View {
  zoom: number;
  x: number;
  y: number;
}

/**
 * Drag to move, wheel or slider to zoom, arrow keys and +/- from the
 * keyboard. The picture always covers the whole circle, so there is no way
 * to end up with an empty edge.
 */
function Cropper({
  ref,
  src,
  onError,
}: {
  ref: Ref<CropperHandle>;
  src: string;
  onError: () => void;
}) {
  const t = useT();
  const viewport = useRef<HTMLDivElement>(null);
  const drag = useRef<{ px: number; py: number; x: number; y: number } | null>(
    null,
  );
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [view, setView] = useState<View>({ zoom: 1, x: 0, y: 0 });

  const base = img
    ? Math.max(VIEW / img.naturalWidth, VIEW / img.naturalHeight)
    : 1;

  // Keeps the picture covering the viewport at whatever zoom `v` asks for.
  const clamp = useCallback(
    (v: View): View => {
      if (!img) return v;
      const zoom = Math.min(MAX_ZOOM, Math.max(1, v.zoom));
      const w = img.naturalWidth * base * zoom;
      const h = img.naturalHeight * base * zoom;
      return {
        zoom,
        x: Math.min(0, Math.max(VIEW - w, v.x)),
        y: Math.min(0, Math.max(VIEW - h, v.y)),
      };
    },
    [img, base],
  );

  useEffect(() => {
    let live = true;
    const image = new Image();
    image.onload = () => {
      if (!live) return;
      const fit = Math.max(
        VIEW / image.naturalWidth,
        VIEW / image.naturalHeight,
      );
      setImg(image);
      setView({
        zoom: 1,
        x: (VIEW - image.naturalWidth * fit) / 2,
        y: (VIEW - image.naturalHeight * fit) / 2,
      });
    };
    image.onerror = () => {
      if (live) onError();
    };
    image.src = src;
    return () => {
      live = false;
    };
  }, [src, onError]);

  /** Zooms keeping the picture's point under `anchor` where it is. */
  const zoomBy = useCallback(
    (next: (zoom: number) => number, anchor = { x: VIEW / 2, y: VIEW / 2 }) => {
      setView((v) => {
        const zoom = Math.min(MAX_ZOOM, Math.max(1, next(v.zoom)));
        const ratio = zoom / v.zoom;
        return clamp({
          zoom,
          x: anchor.x - (anchor.x - v.x) * ratio,
          y: anchor.y - (anchor.y - v.y) * ratio,
        });
      });
    },
    [clamp],
  );

  // A native listener: React's `onWheel` is passive, and without
  // `preventDefault` the wheel would scroll the dialog as well.
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      const rect = el!.getBoundingClientRect();
      zoomBy((z) => z * Math.exp(-e.deltaY * 0.0015), {
        x: e.clientX - rect.left,
        y: e.clientY - rect.top,
      });
    }
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomBy]);

  useImperativeHandle(
    ref,
    () => ({
      exportCrop() {
        if (!img) return null;
        const canvas = document.createElement("canvas");
        canvas.width = OUT;
        canvas.height = OUT;
        const ctx = canvas.getContext("2d");
        if (!ctx) return null;
        ctx.imageSmoothingQuality = "high";
        const scale = base * view.zoom;
        const side = VIEW / scale;
        ctx.drawImage(img, -view.x / scale, -view.y / scale, side, side, 0, 0, OUT, OUT);
        const webp = canvas.toDataURL("image/webp", 0.9);
        // A webview that can't encode WebP hands back PNG under the same
        // call; asking for PNG outright keeps the answer explicit.
        return webp.startsWith("data:image/webp")
          ? webp
          : canvas.toDataURL("image/png");
      },
    }),
    [img, base, view],
  );

  const scale = base * view.zoom;

  return (
    <>
      <div
        ref={viewport}
        tabIndex={0}
        role="img"
        aria-label={t("avatar.cropLabel")}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          drag.current = { px: e.clientX, py: e.clientY, x: view.x, y: view.y };
        }}
        onPointerMove={(e) => {
          const d = drag.current;
          if (!d) return;
          setView((v) =>
            clamp({
              ...v,
              x: d.x + e.clientX - d.px,
              y: d.y + e.clientY - d.py,
            }),
          );
        }}
        onPointerUp={() => (drag.current = null)}
        onPointerCancel={() => (drag.current = null)}
        onKeyDown={(e) => {
          const step = e.shiftKey ? 40 : 10;
          const pan: Record<string, [number, number]> = {
            ArrowLeft: [step, 0],
            ArrowRight: [-step, 0],
            ArrowUp: [0, step],
            ArrowDown: [0, -step],
          };
          if (e.key in pan) {
            e.preventDefault();
            const [dx, dy] = pan[e.key];
            setView((v) => clamp({ ...v, x: v.x + dx, y: v.y + dy }));
          } else if (e.key === "+" || e.key === "=") {
            zoomBy((z) => z * 1.1);
          } else if (e.key === "-") {
            zoomBy((z) => z / 1.1);
          }
        }}
        style={{ width: VIEW, height: VIEW }}
        className="relative shrink-0 cursor-grab touch-none overflow-hidden rounded-xl bg-neutral-900 outline-none select-none focus-visible:ring-2 focus-visible:ring-neutral-500 active:cursor-grabbing"
      >
        {img ? (
          <img
            src={src}
            alt=""
            draggable={false}
            style={{
              left: view.x,
              top: view.y,
              width: img.naturalWidth * scale,
              height: img.naturalHeight * scale,
            }}
            className="pointer-events-none absolute max-w-none"
          />
        ) : (
          <LoaderCircle className="absolute inset-0 m-auto size-6 animate-spin text-neutral-600" />
        )}
        {/* Dims everything outside the circle the avatar will be shown in. */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 rounded-full shadow-[0_0_0_9999px_rgba(0,0,0,0.6)] ring-1 ring-white/30"
        />
      </div>
      <div className="flex w-full max-w-64 items-center gap-2 text-neutral-500">
        <button
          onClick={() => zoomBy((z) => z / 1.25)}
          disabled={!img || view.zoom <= 1}
          aria-label={t("avatar.zoomOut")}
          title={t("avatar.zoomOut")}
          className={`${btn.quiet} size-6`}
        >
          <ZoomOut className="size-4" />
        </button>
        <input
          type="range"
          min={1}
          max={MAX_ZOOM}
          step={0.01}
          value={view.zoom}
          disabled={!img}
          aria-label={t("avatar.zoom")}
          onChange={(e) => {
            const target = Number(e.target.value);
            zoomBy(() => target);
          }}
        />
        <button
          onClick={() => zoomBy((z) => z * 1.25)}
          disabled={!img || view.zoom >= MAX_ZOOM}
          aria-label={t("avatar.zoomIn")}
          title={t("avatar.zoomIn")}
          className={`${btn.quiet} size-6`}
        >
          <ZoomIn className="size-4" />
        </button>
      </div>
    </>
  );
}
