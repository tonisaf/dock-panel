import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";

export interface OpenClawStatus {
  connected: boolean;
  reachable: boolean;
  error: string | null;
}

export function useOpenClawStatus() {
  return useQuery({
    queryKey: ["openclaw-status"],
    queryFn: () => invoke<OpenClawStatus>("openclaw_status"),
    staleTime: 60_000,
  });
}

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

/** The OpenClaw Gateway token; questions go there from the AI tab once it is saved. */
export function OpenClawSettings() {
  const queryClient = useQueryClient();
  const status = useOpenClawStatus().data;
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("openclaw_set_token", { token });
      setToken("");
      await queryClient.invalidateQueries({ queryKey: ["openclaw-status"] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    await invoke("openclaw_disconnect");
    await queryClient.invalidateQueries({ queryKey: ["openclaw-status"] });
  };

  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <p>
        Gateway OpenClaw на этом ПК (<code>127.0.0.1:18789</code>). Токен лежит в{" "}
        <code>~/.openclaw/openclaw.json</code> в WSL (<code>gateway.auth.token</code>); здесь он хранится в диспетчере
        учётных данных Windows. Вопросы к OpenClaw — на вкладке AI, переключатель «OpenClaw».
      </p>
      {status?.connected ? (
        <div className="flex items-center gap-2">
          <span className={status.reachable ? "text-ok" : "text-warn"}>
            {status.reachable ? "Gateway отвечает" : (status.error ?? "Gateway не отвечает")}
          </span>
          <button className={button + " ml-auto"} onClick={disconnect}>
            Отключить
          </button>
        </div>
      ) : (
        <div className="flex gap-2">
          <input
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && connect()}
            placeholder="Токен Gateway"
            spellCheck={false}
            className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 text-[13px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50"
          />
          <button className={button} disabled={busy || !token.trim()} onClick={connect}>
            {busy && <Loader2 className="size-3.5 animate-spin" />} Подключить
          </button>
        </div>
      )}
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}
