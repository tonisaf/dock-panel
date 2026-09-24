import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { motion } from "motion/react";
import { useQueryClient } from "@tanstack/react-query";
import { Check, Laptop, Loader2, MonitorSpeaker, Smartphone, Speaker, Tv, type LucideIcon } from "lucide-react";
import clsx from "clsx";
import { transferTo, useDevices } from "./api";

const ICONS: Record<string, LucideIcon> = {
  Computer: Laptop,
  Smartphone,
  Speaker,
  TV: Tv,
  CastVideo: Tv,
  CastAudio: Speaker,
  AVR: Speaker,
  STB: Tv,
  AudioDongle: Speaker,
  GameConsole: Tv,
  Automobile: Speaker,
};

/** Spotify Connect: lists the account's devices and moves playback to the one clicked. */
export function DevicePicker({ className }: { className?: string }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [switching, setSwitching] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { data: devices, isPending } = useDevices(open);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  // Below the button, kept inside the window.
  useLayoutEffect(() => {
    if (!open || !buttonRef.current || !menuRef.current) return;
    const b = buttonRef.current.getBoundingClientRect();
    const { width, height } = menuRef.current.getBoundingClientRect();
    setPos({
      x: Math.max(8, Math.min(b.right - width, window.innerWidth - width - 8)),
      y: b.bottom + height + 12 > window.innerHeight ? b.top - height - 4 : b.bottom + 4,
    });
  }, [open, devices, error]);

  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      const t = e.target as Node;
      if (!menuRef.current?.contains(t) && !buttonRef.current?.contains(t)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("wheel", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("wheel", close);
    };
  }, [open]);

  const pick = async (id: string) => {
    setSwitching(id);
    setError(null);
    try {
      await transferTo(id);
      setOpen(false);
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ["media"] });
        queryClient.invalidateQueries({ queryKey: ["spotify-devices"] });
      }, 800);
    } catch (e) {
      setError(String(e));
    } finally {
      setSwitching(null);
    }
  };

  const item = "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-ink/10";
  return (
    <>
      <button
        ref={buttonRef}
        onClick={() => {
          setError(null);
          setOpen(!open);
        }}
        title="Устройство Spotify"
        className={clsx(
          "grid size-7 place-items-center rounded-full text-fg-muted transition-colors hover:bg-ink/10 hover:text-fg",
          open && "bg-ink/10 text-fg",
          className,
        )}
      >
        <MonitorSpeaker className="size-4" />
      </button>
      {open &&
        createPortal(
          <motion.div
            ref={menuRef}
            initial={{ opacity: 0, scale: 0.96 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ duration: 0.1 }}
            style={{ left: pos.x, top: pos.y }}
            className="fixed z-50 w-60 rounded-xl border border-ink/10 bg-popover p-1 text-fg shadow-2xl shadow-black/50"
          >
            <div className="px-2.5 pt-1 pb-1.5 text-[11px] text-fg-subtle">Играть на устройстве</div>
            {isPending && <div className="px-2.5 py-1.5 text-[12px] text-fg-subtle">Ищу устройства…</div>}
            {devices?.length === 0 && (
              <div className="px-2.5 py-1.5 text-[12px] leading-relaxed text-fg-subtle">
                Нет доступных устройств. Откройте Spotify на компьютере, телефоне или колонке.
              </div>
            )}
            {devices?.map((d) => {
              const Icon = ICONS[d.kind] ?? Speaker;
              return (
                <button key={d.id} className={item} disabled={!!switching} onClick={() => !d.active && pick(d.id)}>
                  <Icon className={clsx("size-4", d.active ? "text-[#1ed760]" : "text-fg-muted")} />
                  <span className={clsx("min-w-0 flex-1 truncate", d.active && "text-[#1ed760]")}>{d.name}</span>
                  {switching === d.id ? (
                    <Loader2 className="size-3.5 animate-spin text-fg-subtle" />
                  ) : (
                    d.active && <Check className="size-3.5 text-[#1ed760]" />
                  )}
                </button>
              );
            })}
            {error && <div className="px-2.5 py-1.5 text-[12px] text-warn">{error}</div>}
          </motion.div>,
          document.body,
        )}
    </>
  );
}
