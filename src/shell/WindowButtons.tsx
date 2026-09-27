import { ChevronsLeft, ChevronsRight, Maximize2, Minimize2, Pin } from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../store";
import { usePanelSettings } from "../lib/panelWidth";

const button = "grid size-11 shrink-0 place-items-center rounded-xl border transition-colors";
const idle = "border-stroke bg-surface text-fg-muted hover:bg-surface-hover hover:text-fg";

/** Pin the panel open, and hide it (the only way out while pinned, besides the hotkey). */
export function WindowButtons() {
  const { pinned, setPinned, hide, full, setFull } = usePanelStore();
  const edge = usePanelSettings().edge;
  const HideIcon = edge === "right" ? ChevronsRight : ChevronsLeft;

  return (
    <>
      <button
        onClick={() => setFull(!full)}
        title={full ? "Обычный размер" : "Во весь экран: главная слева, остальные вкладки справа"}
        aria-pressed={full}
        className={clsx(button, full ? "border-accent/60 bg-accent/15 text-accent" : idle)}
      >
        {full ? <Minimize2 className="size-4" /> : <Maximize2 className="size-4" />}
      </button>
      <button
        onClick={() => setPinned(!pinned)}
        title={pinned ? "Открепить: панель снова прячется при клике мимо" : "Закрепить: панель не прячется, пока не нажать «Скрыть»"}
        aria-pressed={pinned}
        className={clsx(button, pinned ? "border-accent/60 bg-accent/15 text-accent" : idle)}
      >
        <Pin className="size-4" fill={pinned ? "currentColor" : "none"} />
      </button>
      <button onClick={hide} title="Скрыть панель" className={clsx(button, idle)}>
        <HideIcon className="size-4" />
      </button>
    </>
  );
}
