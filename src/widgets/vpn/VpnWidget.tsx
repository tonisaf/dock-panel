import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Loader2, ShieldCheck, ShieldOff } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";

interface VpnInfo {
  id: "openvpn" | "amnezia";
  name: string;
  installed: boolean;
  connected: boolean;
  detail: string | null;
  canToggle: boolean;
}

/** How long to keep polling fast after a toggle while the tunnel settles. */
const SETTLE_MS = 20_000;

function hint(v: VpnInfo) {
  if (v.id === "openvpn") return v.connected ? "Отключить" : "Откроется OpenVPN Connect для ввода пароля";
  if (v.connected) return "Отключить: Windows попросит подтверждение (UAC)";
  // Amnezia removes its tunnel service on disconnect; then only its own window can connect.
  return v.canToggle ? "Подключить: Windows попросит подтверждение (UAC)" : "Откроется AmneziaVPN: нажмите «Подключиться»";
}

function Switch({ on, busy, disabled, onClick, title }: { on: boolean; busy: boolean; disabled: boolean; onClick: () => void; title: string }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={disabled || busy}
      onClick={onClick}
      title={title}
      className={clsx(
        "flex h-6 w-11 shrink-0 items-center rounded-full border px-0.5 transition-colors disabled:opacity-50",
        on ? "justify-end border-emerald-400 bg-emerald-400" : "justify-start border-ink/25",
      )}
    >
      <span className={clsx("grid size-4 place-items-center rounded-full", on ? "bg-black/80" : "bg-ink/70")}>
        {busy && <Loader2 className={clsx("size-3 animate-spin", on ? "text-white" : "text-black")} />}
      </span>
    </button>
  );
}

export function VpnWidget() {
  const queryClient = useQueryClient();
  const [settleUntil, setSettleUntil] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { data } = useQuery({
    queryKey: ["vpn"],
    queryFn: () => invoke<VpnInfo[]>("vpn_status"),
    refetchInterval: () => (Date.now() < settleUntil ? 1000 : 5000),
    staleTime: 0,
  });

  const vpns = (data ?? []).filter((v) => v.installed);
  if (data && vpns.length === 0) return null;

  const toggle = async (v: VpnInfo) => {
    setBusy(v.id);
    setError(null);
    try {
      const outcome = await invoke<"done" | "opened">("vpn_toggle", { id: v.id, connect: !v.connected });
      // The password prompt / Amnezia window needs the screen.
      if (outcome === "opened") usePanelStore.getState().setOpen(false);
      setSettleUntil(Date.now() + SETTLE_MS);
      queryClient.invalidateQueries({ queryKey: ["vpn"] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const open = (id: string) => {
    invoke("vpn_open", { id }).catch((e) => setError(String(e)));
    usePanelStore.getState().setOpen(false);
  };

  return (
    <Card title="VPN" icon={ShieldCheck}>
      <div className="flex flex-col gap-1">
        {vpns.map((v) => {
          const Icon = v.connected ? ShieldCheck : ShieldOff;
          return (
            <div key={v.id} className="group -mx-1.5 flex items-center gap-2.5 rounded-xl px-1.5 py-1.5 hover:bg-surface">
              <div
                className={clsx(
                  "grid size-8 shrink-0 place-items-center rounded-lg",
                  v.connected ? "bg-emerald-400/15 text-ok" : "bg-ink/6 text-fg-subtle",
                )}
              >
                <Icon className="size-4" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5 text-[13.5px]">
                  {v.name}
                  <button
                    onClick={() => open(v.id)}
                    title={`Открыть ${v.name}`}
                    className="text-fg-subtle opacity-0 transition-opacity group-hover:opacity-100 hover:text-fg"
                  >
                    <ExternalLink className="size-3" />
                  </button>
                </div>
                <div className={clsx("truncate text-[11.5px]", v.connected ? "text-ok/90" : "text-fg-subtle")}>
                  {v.connected ? "Подключён" : "Отключён"}
                  {v.detail && <span className="text-fg-subtle"> · {v.detail}</span>}
                </div>
              </div>
              <Switch
                on={v.connected}
                busy={busy === v.id}
                disabled={!v.canToggle && v.connected}
                onClick={() => toggle(v)}
                title={hint(v)}
              />
            </div>
          );
        })}
      </div>
      {error && <p className="mt-1.5 text-[12px] leading-relaxed text-warn">{error}</p>}
    </Card>
  );
}
