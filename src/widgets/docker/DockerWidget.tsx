import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Container as ContainerIcon, Loader2, Play, RotateCw, Square } from "lucide-react";
import clsx from "clsx";
import { WidgetState } from "../../components/WidgetState";
import { Card } from "../../components/Card";
import { health, shortStatus, stateTone, summary } from "./format";

interface Container {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  ports: string[];
  project: string | null;
}

interface Overview {
  running: boolean;
  containers: Container[];
  error: string | null;
}

type Action = "start" | "stop" | "restart";

function useOverview() {
  return useQuery({
    queryKey: ["docker-overview"],
    queryFn: () => invoke<Overview>("docker_overview"),
    staleTime: 5_000,
    refetchInterval: 10_000,
  });
}

const iconButton =
  "grid size-8 shrink-0 place-items-center rounded-lg border border-stroke text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

const dot = { ok: "bg-ok", warn: "bg-warn", off: "bg-fg-subtle/40" } as const;

/** Containers with their state; start, stop and restart from here. */
export function DockerWidget() {
  const queryClient = useQueryClient();
  const { data, error: queryError, isFetching, refetch } = useOverview();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showStopped, setShowStopped] = useState(false);

  const act = async (c: Container, action: Action) => {
    setBusy(c.id);
    setError(null);
    try {
      await invoke("docker_action", { id: c.id, action });
    } catch (e) {
      setError(`${c.name}: ${String(e)}`);
    } finally {
      setBusy(null);
      await queryClient.invalidateQueries({ queryKey: ["docker-overview"] });
    }
  };

  if (!data) return (
    <Card title="Docker" icon={ContainerIcon}>
      <WidgetState kind={queryError ? "error" : "loading"} text={queryError ? String(queryError) : undefined} onRetry={() => void refetch()} retrying={isFetching} />
    </Card>
  );
  const up = data.containers.filter((c) => c.state === "running");
  const rest = data.containers.filter((c) => c.state !== "running");

  const row = (c: Container) => {
    const tone = stateTone(c.state);
    const check = health(c.status);
    const working = busy === c.id;
    const details = [c.image, shortStatus(c.status), c.ports.length ? c.ports.map((p) => `:${p}`).join(" ") : null]
      .filter(Boolean)
      .join(" · ");
    return (
      <div key={c.id} className="flex items-center gap-2 rounded-xl bg-ink/5 px-2.5 py-2">
        <span className={clsx("size-2 shrink-0 rounded-full", dot[tone])} title={c.state} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-[13px]">
            <span className="truncate font-medium">{c.name}</span>
            {check && check !== "healthy" && (
              <span className={clsx("shrink-0 text-[11px]", check === "unhealthy" ? "text-warn" : "text-fg-subtle")}>
                {check === "unhealthy" ? "не здоров" : "запускается"}
              </span>
            )}
          </div>
          <div className="truncate text-[11.5px] text-fg-subtle" title={c.status}>
            {details}
          </div>
          {c.project && <div className="truncate text-[11px] text-fg-subtle/80">{c.project}</div>}
        </div>
        {working ? (
          <Loader2 className="size-4 shrink-0 animate-spin text-fg-subtle" />
        ) : (
          <>
            {tone !== "off" && (
              <button className={iconButton} title="Перезапустить" disabled={busy !== null} onClick={() => act(c, "restart")}>
                <RotateCw className="size-3.5" />
              </button>
            )}
            <button
              className={iconButton}
              title={c.state === "running" || c.state === "restarting" ? "Остановить" : "Запустить"}
              disabled={busy !== null}
              onClick={() => act(c, c.state === "running" || c.state === "restarting" ? "stop" : "start")}
            >
              {c.state === "running" || c.state === "restarting" ? (
                <Square className="size-3.5" />
              ) : (
                <Play className="size-3.5" />
              )}
            </button>
          </>
        )}
      </div>
    );
  };

  return (
    <Card
      title="Docker"
      icon={ContainerIcon}
      action={
        data.running && (
          <span className="text-[11.5px] text-fg-subtle">{summary(up.length, data.containers.length)}</span>
        )
      }
    >
      {data.error ? (
        <WidgetState kind="error" text={data.error} onRetry={() => void refetch()} retrying={isFetching} />
      ) : data.containers.length === 0 ? (
        <WidgetState kind="empty" text="Контейнеров нет" />
      ) : (
        <>
          <div className="flex flex-col gap-1">
            {up.length === 0 && <p className="text-[12px] leading-relaxed text-fg-subtle">Ничего не запущено.</p>}
            {up.map(row)}
          </div>
          {rest.length > 0 && (
            <div className="mt-2.5 border-t border-stroke pt-2">
              <button
                onClick={() => setShowStopped((v) => !v)}
                className="flex w-full items-center gap-1 text-[11.5px] text-fg-subtle hover:text-fg"
              >
                <ChevronDown className={clsx("size-3.5 transition-transform", !showStopped && "-rotate-90")} />
                Остановленные ({rest.length})
              </button>
              {showStopped && <div className="mt-1.5 flex flex-col gap-1">{rest.map(row)}</div>}
            </div>
          )}
        </>
      )}
      {error && <p className="mt-2 text-[12px] leading-relaxed text-warn">{error}</p>}
    </Card>
  );
}
