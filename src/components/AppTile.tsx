import { forwardRef, type MouseEvent } from "react";
import { Pin } from "lucide-react";
import clsx from "clsx";
import { launchApp, type AppEntry } from "../lib/apps";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { AppIcon } from "./AppIcon";

function useOpenMenu(id: string) {
  const setMenu = usePanelStore((s) => s.setMenu);
  return (e: MouseEvent) => {
    e.preventDefault();
    setMenu({ appId: id, x: e.clientX, y: e.clientY });
  };
}

/** Launchpad-style grid cell. */
export function AppTile({ app }: { app: AppEntry }) {
  const onContextMenu = useOpenMenu(app.id);
  return (
    <button
      onClick={() => launchApp(app.id)}
      onContextMenu={onContextMenu}
      title={app.name}
      className="group flex flex-col items-center gap-1.5 rounded-xl px-1 pt-2.5 pb-2 outline-none transition-colors hover:bg-surface-hover focus-visible:bg-surface-hover active:scale-[0.97]"
    >
      <div className="transition-transform duration-150 group-hover:-translate-y-0.5">
        <AppIcon id={app.id} size={40} />
      </div>
      <span className="line-clamp-2 w-full text-center text-[11.5px] leading-tight text-fg-muted group-hover:text-fg">
        {app.name}
      </span>
    </button>
  );
}

/** Search result row. */
export const AppRow = forwardRef<HTMLButtonElement, { app: AppEntry; active: boolean; onHover: () => void }>(
  function AppRow({ app, active, onHover }, ref) {
    const onContextMenu = useOpenMenu(app.id);
    const pinned = usePrefs((s) => s.pinned.includes(app.id));
    return (
      <button
        ref={ref}
        onClick={() => launchApp(app.id)}
        onContextMenu={onContextMenu}
        onMouseMove={onHover}
        className={clsx(
          "flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left outline-none transition-colors",
          active ? "bg-ink/10" : "hover:bg-surface",
        )}
      >
        <AppIcon id={app.id} size={32} />
        <span className="min-w-0 flex-1 truncate text-[14px]">{app.name}</span>
        {pinned && <Pin className="size-3.5 text-fg-subtle" />}
        {active && <kbd className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Enter</kbd>}
      </button>
    );
  },
);

export function AppGrid({ apps }: { apps: AppEntry[] }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-1">
      {apps.map((app) => (
        <AppTile key={app.id} app={app} />
      ))}
    </div>
  );
}
