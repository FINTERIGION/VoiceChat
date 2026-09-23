import { useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

/** The URL the backend's `avatar://` protocol serves a stored picture at. */
export function avatarSrc(avatarPath: string): string {
  return convertFileSrc(avatarPath, "avatar");
}

/**
 * Stand-in colours for a character without a picture, picked by id so the
 * same character always wears the same one, list to header to editor.
 */
const TINTS = [
  "bg-emerald-500/20 text-emerald-300",
  "bg-sky-500/20 text-sky-300",
  "bg-violet-500/20 text-violet-300",
  "bg-amber-500/20 text-amber-300",
  "bg-rose-500/20 text-rose-300",
  "bg-teal-500/20 text-teal-300",
];

function tintFor(seed: string): string {
  let hash = 0;
  for (const ch of seed) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return TINTS[Math.abs(hash) % TINTS.length];
}

const SIZE = {
  xs: "size-6 text-[11px]",
  sm: "size-8 text-sm",
  md: "size-9 text-sm",
  lg: "size-16 text-2xl",
  xl: "size-20 text-3xl",
} as const;

/**
 * A character's picture, or the first letter of their name on a tint when
 * they have none — or when the file is gone (restored by a build that
 * didn't carry pictures, say), which the `<img>` finds out by failing.
 *
 * `current` adds the green dot the app marks the live character with.
 */
export default function Avatar({
  id,
  name,
  avatarPath,
  size = "md",
  current = false,
  className = "",
}: {
  /** Seeds the fallback tint; a new character with no id yet passes "". */
  id: string;
  name: string;
  avatarPath: string | null;
  size?: keyof typeof SIZE;
  current?: boolean;
  className?: string;
}) {
  // Keyed on the path, so a picture that failed doesn't stop its
  // replacement from being tried.
  const [failed, setFailed] = useState<string | null>(null);
  const showImage = avatarPath !== null && failed !== avatarPath;
  // `Array.from` so a name starting with an emoji or a surrogate pair isn't
  // cut in half.
  const initial = Array.from(name.trim())[0]?.toUpperCase() ?? "?";

  return (
    <span className={`relative inline-flex shrink-0 ${SIZE[size]} ${className}`}>
      {showImage ? (
        <img
          src={avatarSrc(avatarPath)}
          alt=""
          draggable={false}
          onError={() => setFailed(avatarPath)}
          className="size-full rounded-full bg-neutral-800 object-cover"
        />
      ) : (
        <span
          aria-hidden
          className={`flex size-full items-center justify-center rounded-full font-medium select-none ${tintFor(id || name)}`}
        >
          {initial}
        </span>
      )}
      {current && (
        <span
          aria-hidden
          className={`absolute right-0 bottom-0 rounded-full bg-emerald-400 ring-2 ring-neutral-950 ${
            size === "lg" || size === "xl" ? "size-3.5" : "size-2.5"
          }`}
        />
      )}
    </span>
  );
}
