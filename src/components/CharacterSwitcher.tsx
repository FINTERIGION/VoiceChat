import { useEffect, useId, useRef, useState } from "react";
import { Check, ChevronDown, Settings2 } from "lucide-react";
import { useT } from "../lib/i18n";
import { ipc } from "../lib/ipc";
import type { Character } from "../lib/types";
import { interactive } from "../lib/ui";
import Avatar from "./Avatar";

/**
 * The Chat header's character name, doubling as the way to switch: it
 * opens a menu of every character, with the way to the Characters tab at
 * the bottom.
 *
 * Switching needs no confirmation. The conversation being left is saved to
 * the history like any other that ends, so nothing is lost by it; the Chat
 * tab says as much once the switch lands.
 */
export default function CharacterSwitcher({
  current,
  onManage,
}: {
  /** `undefined` while loading, `null` when there is no current character. */
  current: Character | null | undefined;
  onManage: () => void;
}) {
  const t = useT();
  const menuId = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [characters, setCharacters] = useState<Character[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Read fresh on every open: characters are created, renamed and deleted
  // on another tab, and nothing announces it.
  useEffect(() => {
    if (!open) return;
    setError(null);
    ipc
      .listCharacters()
      .then((list) => {
        setCharacters(list);
        // Starts on the current character, the way a select opens on its
        // value; after the render that puts the list on screen.
        requestAnimationFrame(() => {
          const items = menuItems();
          (items.find((b) => b.getAttribute("aria-checked") === "true") ??
            items[0])?.focus();
        });
      })
      .catch((e) => setError(String(e)));
  }, [open, current?.id]);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(e: PointerEvent) {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    }
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  async function pick(id: string) {
    setOpen(false);
    if (id === current?.id) return;
    try {
      await ipc.switchCharacter(id);
    } catch (e) {
      setError(String(e));
    }
  }

  function menuItems(): HTMLButtonElement[] {
    return [
      ...(root.current?.querySelectorAll<HTMLButtonElement>(
        '[role^="menuitem"]',
      ) ?? []),
    ];
  }

  function onMenuKeyDown(e: React.KeyboardEvent) {
    const buttons = menuItems();
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null = null;
    if (e.key === "ArrowDown") next = (at + 1) % buttons.length;
    else if (e.key === "ArrowUp")
      next = (at - 1 + buttons.length) % buttons.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = buttons.length - 1;
    else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      trigger.current?.focus();
      return;
    } else if (e.key === "Tab") {
      setOpen(false);
      return;
    }
    if (next !== null) {
      e.preventDefault();
      buttons[next]?.focus();
    }
  }

  const name =
    current === undefined
      ? " "
      : (current?.name ?? t("chat.noCharacter"));

  return (
    <div ref={root} className="relative min-w-0">
      <button
        ref={trigger}
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        title={t("chat.switchCharacter")}
        // Shown whole, never cut short: `character::NAME_MAX_WIDTH` keeps a
        // name to one line, and one saved before that limit existed wraps.
        // No `max-w-full` + `truncate` here — on a <button> that combination
        // lets the column shrink to the width of the status line below.
        className={`${interactive} -ml-1.5 flex items-center gap-1 rounded-lg px-1.5 py-0.5 text-left text-neutral-100 hover:bg-neutral-800/70`}
      >
        <span className="text-sm font-medium break-words">{name}</span>
        <ChevronDown
          className={`size-3.5 shrink-0 text-neutral-500 transition-transform duration-200 ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>

      {error && !open && (
        <p role="alert" className="mt-0.5 text-xs text-red-400">
          {error}
        </p>
      )}

      {open && (
        <div
          id={menuId}
          role="menu"
          aria-label={t("chat.switchCharacter")}
          onKeyDown={onMenuKeyDown}
          className="absolute top-full left-0 z-40 mt-1.5 w-72 overflow-hidden rounded-xl border border-neutral-800 bg-neutral-950 p-1 shadow-2xl shadow-black/60"
        >
          {error && (
            <p role="alert" className="px-2.5 py-2 text-xs text-red-400">
              {error}
            </p>
          )}
          <div className="max-h-72 overflow-y-auto">
            {characters.map((c) => {
              const isCurrent = c.id === current?.id;
              return (
                <button
                  key={c.id}
                  role="menuitemradio"
                  aria-checked={isCurrent}
                  onClick={() => pick(c.id)}
                  className={`${interactive} flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left hover:bg-neutral-800/80 focus-visible:bg-neutral-800/80 focus-visible:ring-0 ${
                    isCurrent ? "text-neutral-100" : "text-neutral-300"
                  }`}
                >
                  <Avatar
                    id={c.id}
                    name={c.name}
                    avatarPath={c.avatar_path}
                    size="sm"
                  />
                  <span className="min-w-0 flex-1 text-sm break-words">
                    {c.name}
                  </span>
                  {isCurrent && (
                    <Check className="size-4 shrink-0 text-emerald-400" />
                  )}
                </button>
              );
            })}
          </div>
          <div className="my-1 border-t border-neutral-800" />
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onManage();
            }}
            className={`${interactive} flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm text-neutral-400 hover:bg-neutral-800/80 hover:text-neutral-100 focus-visible:bg-neutral-800/80 focus-visible:ring-0`}
          >
            <Settings2 className="size-4" />
            {t("chat.manageCharacters")}
          </button>
        </div>
      )}
    </div>
  );
}
