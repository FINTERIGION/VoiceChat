import { useState } from "react";
import { CircleArrowUp } from "lucide-react";
import { useT } from "../lib/i18n";
import { btn } from "../lib/ui";
import { useUpdates } from "../lib/update";

/**
 * The strip under the tabs announcing a newer version. It only points the
 * way: the release notes and the install button are in Settings → About.
 * "Later" hides it for that version; a newer one brings it back.
 */
export default function UpdateBanner({
  hidden,
  onView,
}: {
  /** Set while About is on screen, which already says all this. */
  hidden: boolean;
  onView: () => void;
}) {
  const t = useT();
  const { state } = useUpdates();
  const [dismissed, setDismissed] = useState<string | null>(null);

  if (hidden || state.kind !== "available") return null;
  const { version } = state.info;
  if (version === dismissed) return null;

  return (
    <div
      role="status"
      className="flex items-center gap-3 border-b border-neutral-800 bg-emerald-500/10 px-4 py-2 text-sm"
    >
      <CircleArrowUp className="size-4 shrink-0 text-emerald-400" />
      <span className="min-w-0 flex-1 text-neutral-200">
        {t("update.banner", { version })}
      </span>
      <button onClick={onView} className={`${btn.solid} px-3 py-1 text-xs`}>
        {t("update.view")}
      </button>
      <button
        onClick={() => setDismissed(version)}
        className={`${btn.quiet} px-2 py-1 text-xs`}
      >
        {t("update.later")}
      </button>
    </div>
  );
}
