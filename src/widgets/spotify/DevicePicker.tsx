import { useCallback, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Check, Laptop, Loader2, MonitorSpeaker, Smartphone, Speaker, Tv, type LucideIcon } from "lucide-react";
import clsx from "clsx";
import { transferTo, useDevices } from "./api";
import { AnchoredMenu, menuItem } from "./Menu";

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
  const close = useCallback(() => setOpen(false), []);

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
      {open && (
        <AnchoredMenu anchor={buttonRef} onClose={close} className="w-60">
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
              <button key={d.id} className={menuItem} disabled={!!switching} onClick={() => !d.active && pick(d.id)}>
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
        </AnchoredMenu>
      )}
    </>
  );
}
