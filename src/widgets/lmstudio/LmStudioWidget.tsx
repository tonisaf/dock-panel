import { useIntegrations, llmName } from "../../lib/integrations";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Cpu, Loader2, Power, Upload } from "lucide-react";
import clsx from "clsx";
import { WidgetState } from "../../components/WidgetState";
import { Card } from "../../components/Card";
import { formatContext, formatSize, unloadsIn } from "./format";

interface Model {
  key: string;
  identifier: string | null;
  name: string;
  kind: string;
  params: string | null;
  quantization: string | null;
  sizeBytes: number;
  contextLength: number | null;
  maxContextLength: number | null;
  ttlMs: number | null;
  lastUsedMs: number | null;
  status: string | null;
  vision: boolean;
  toolUse: boolean;
}

interface Overview {
  running: boolean;
  loaded: Model[];
  available: Model[];
  error: string | null;
}

const TTL_KEY = "lmstudio.ttl";
/** Auto-unload choices, in minutes; 0 keeps the model loaded. */
const TTLS = [
  { minutes: 0, label: "нет" },
  { minutes: 10, label: "10 мин" },
  { minutes: 30, label: "30 мин" },
  { minutes: 60, label: "1 ч" },
];

function savedTtl(): number {
  try {
    const n = Number(localStorage.getItem(TTL_KEY));
    return TTLS.some((t) => t.minutes === n) ? n : 0;
  } catch {
    return 0;
  }
}

export function useLmStudioOverview() {
  const { llm: provider, loaded } = useIntegrations();
  return useQuery({
    enabled: loaded,
    queryKey: ["lmstudio-overview", provider],
    queryFn: () => invoke<Overview>("lmstudio_overview"),
    staleTime: 5_000,
    refetchInterval: 10_000,
  });
}

const smallButton =
  "flex shrink-0 items-center gap-1 rounded-lg border border-stroke px-2 py-1 text-[11.5px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

function details(m: Model) {
  return [m.params, m.quantization, formatSize(m.sizeBytes), m.contextLength ? `контекст ${formatContext(m.contextLength)}` : null]
    .filter(Boolean)
    .join(" · ");
}

/** What LM Studio has loaded, what is on disk, and the server switch; load and unload from here. */
export function LmStudioWidget() {
  const provider = useIntegrations((s) => s.llm);
  const queryClient = useQueryClient();
  const { data, error: queryError, isFetching, refetch } = useLmStudioOverview();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ttl, setTtl] = useState(savedTtl);
  const [showAll, setShowAll] = useState(false);

  const run = async (id: string, command: string, args: Record<string, unknown>) => {
    setBusy(id);
    setError(null);
    try {
      await invoke(command, args);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      await queryClient.invalidateQueries({ queryKey: ["lmstudio-overview"] });
    }
  };

  const chooseTtl = (minutes: number) => {
    setTtl(minutes);
    try {
      localStorage.setItem(TTL_KEY, String(minutes));
    } catch {
      // Storage can be unavailable; the choice just doesn't stick.
    }
  };

  if (!data) return <Card title={llmName()} icon={Cpu}><WidgetState kind={queryError ? "error" : "loading"} text={queryError ? String(queryError) : undefined} onRetry={() => void refetch()} retrying={isFetching} /></Card>;
  const now = Date.now();
  const chat = data.available.filter((m) => m.kind !== "embedding");
  const power = (
    <button
      onClick={() => run("server", "lmstudio_server", { start: !data.running })}
      disabled={busy === "server"}
      aria-pressed={data.running}
      title={data.running ? "Остановить сервер" : "Запустить сервер"}
      className={clsx(
        "grid size-8 place-items-center rounded-lg border transition-colors disabled:opacity-50",
        data.running ? "border-accent/40 bg-accent/15 text-accent" : "border-stroke text-fg-subtle hover:bg-ink/8 hover:text-fg",
      )}
    >
      {busy === "server" ? <Loader2 className="size-3.5 animate-spin" /> : <Power className="size-3.5" />}
    </button>
  );

  return (
    <Card title={llmName()} icon={Cpu} action={power}>
      <p className={clsx("text-[12px]", data.running ? "text-ok" : "text-warn")}>
        {data.running ? `Сервер запущен · ${provider === "ollama" ? "127.0.0.1:11434" : "127.0.0.1:1234"}` : "Сервер не запущен"}
      </p>

      {data.error ? (
        <WidgetState kind="error" text={data.error} onRetry={() => void refetch()} retrying={isFetching} />
      ) : (
        <>
          <div className="mt-2 flex flex-col gap-1">
            {data.loaded.length === 0 && (
              <p className="text-[12px] leading-relaxed text-fg-subtle">
                Модель не загружена: первый запрос из панели загрузит её сам.
              </p>
            )}
            {data.loaded.map((m) => {
              const left = unloadsIn(m.ttlMs, m.lastUsedMs, now);
              return (
                <div key={m.identifier ?? m.key} className="flex items-center gap-2 rounded-xl bg-ink/5 px-2.5 py-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5 text-[13px]">
                      <span className="truncate font-medium">{m.name}</span>
                      <span className={clsx("shrink-0 text-[11px]", m.status === "generating" ? "text-accent" : "text-fg-subtle")}>
                        {m.status === "generating" ? "отвечает…" : "свободна"}
                      </span>
                    </div>
                    <div className="truncate text-[11.5px] text-fg-subtle">{details(m)}</div>
                    {left && <div className="text-[11.5px] text-fg-subtle">выгрузится через {left}</div>}
                  </div>
                  <button
                    className={smallButton}
                    disabled={busy !== null}
                    onClick={() => run(m.key, "lmstudio_unload", { identifier: m.identifier })}
                  >
                    {busy === m.key ? <Loader2 className="size-3 animate-spin" /> : <Upload className="size-3" />} Выгрузить
                  </button>
                </div>
              );
            })}
          </div>

          {chat.length > 0 && (
            <div className="mt-2.5 border-t border-stroke pt-2">
              <button
                onClick={() => setShowAll((v) => !v)}
                className="flex w-full items-center gap-1 text-[11.5px] text-fg-subtle hover:text-fg"
              >
                <ChevronDown className={clsx("size-3.5 transition-transform", !showAll && "-rotate-90")} />
                На диске ({chat.length})
              </button>
              {showAll && (
                <div className="mt-1.5 flex flex-col gap-1">
                  {chat.map((m) => (
                    <div key={m.key} className="flex items-center gap-2 px-1">
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px]">{m.name}</div>
                        <div className="truncate text-[11.5px] text-fg-subtle">{details(m)}</div>
                      </div>
                      <button
                        className={smallButton}
                        disabled={busy !== null || !data.running}
                        onClick={() => run(m.key, "lmstudio_load", { key: m.key, ttlMinutes: ttl || null })}
                      >
                        {busy === m.key && <Loader2 className="size-3 animate-spin" />} Загрузить
                      </button>
                    </div>
                  ))}
                  <div className="mt-1 flex items-center gap-1.5 px-1 text-[11.5px] text-fg-subtle">
                    {provider === "ollama" ? "Удерживать после загрузки:" : "Выгружать при простое:"}
                    {TTLS.map((t) => (
                      <button
                        key={t.minutes}
                        onClick={() => chooseTtl(t.minutes)}
                        aria-pressed={ttl === t.minutes}
                        className={clsx(
                          "rounded-md px-1.5 py-0.5",
                          ttl === t.minutes ? "bg-accent/15 text-accent" : "hover:bg-ink/8 hover:text-fg",
                        )}
                      >
                        {t.label}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </>
      )}
      {error && <p className="mt-2 text-[12px] leading-relaxed text-warn">{error}</p>}
    </Card>
  );
}
