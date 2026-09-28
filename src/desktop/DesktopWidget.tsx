import { useEffect, useLayoutEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppearance } from "../lib/appearance";
import { useDataEvents } from "../lib/dataEvents";
import { useHoldToLift } from "../lib/hold";
import { usePrefs } from "../lib/prefs";
import { WIDGETS } from "../widgets/registry";

/**
 * One home widget alone in its own window on the desktop. The window follows
 * the widget's height; press and hold moves it, right click opens its menu.
 */
export function DesktopWidget({ id }: { id: string }) {
  useAppearance();
  useDataEvents();
  const ref = useRef<HTMLDivElement>(null);
  const def = WIDGETS.find((w) => w.id === id);
  const opacity = usePrefs((s) => s.deskOpacity);
  const blur = usePrefs((s) => s.deskBlur);
  const width = usePrefs((s) => s.widgetWidth);
  // The observer below reads the latest width.
  const widthRef = useRef(width);
  widthRef.current = width;

  useEffect(() => {
    document.documentElement.style.setProperty("--desk-alpha", String(opacity / 100));
  }, [opacity]);
  useEffect(() => {
    invoke("desktop_backdrop", { blur }).catch(console.error);
  }, [blur]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () =>
      invoke("desktop_fit", { width: widthRef.current, height: Math.ceil(el.getBoundingClientRect().height) }).catch(
        console.error,
      );
    fit();
    // Height follows the content; a new width (from the settings) is sent right away.
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [width]);

  const hold = useHoldToLift(() => invoke("desktop_drag").catch(console.error));

  if (!def) return null;
  const Widget = def.component;
  return (
    <div
      ref={ref}
      className="desk-widget"
      onPointerDown={(e) => {
        // The grid overlay takes a moment to start; begin before the hold ends.
        if (e.button === 0) invoke("desktop_grid_prepare").catch(console.error);
        hold(e, id);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        invoke("desktop_menu").catch(console.error);
      }}
    >
      {/* A widget with nothing to show (say, VPN without clients) still needs a body to hold on to. */}
      <div
        data-empty={`${def.title} — сейчас пусто`}
        className="empty:flex empty:h-14 empty:items-center empty:px-3.5 empty:text-[12px] empty:text-fg-subtle empty:before:content-[attr(data-empty)]"
      >
        <Widget />
      </div>
    </div>
  );
}
