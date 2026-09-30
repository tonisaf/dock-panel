import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface LmStudioStatus {
  reachable: boolean;
  models: string[];
  model: string | null;
  error: string | null;
}

export function useLmStudioStatus() {
  return useQuery({
    queryKey: ["lmstudio-status"],
    queryFn: () => invoke<LmStudioStatus>("lmstudio_status"),
    staleTime: 30_000,
  });
}

const button =
  "rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg";

/** The local LM Studio server; questions go there from the AI tab with the «LM Studio» switch. */
export function LmStudioSettings() {
  const queryClient = useQueryClient();
  const query = useLmStudioStatus();
  const status = query.data;

  const pick = async (model: string) => {
    await invoke("lmstudio_set_model", { model });
    await queryClient.invalidateQueries({ queryKey: ["lmstudio-status"] });
  };

  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <p>
        Локальный сервер LM Studio (<code>127.0.0.1:1234</code>): вкладка Developer → Start Server. Вопросы — на вкладке
        AI, переключатель «LM Studio».
      </p>
      <div className="flex items-center gap-2">
        <span className={status?.reachable ? "text-ok" : "text-warn"}>
          {status?.reachable ? "Сервер отвечает" : (status?.error ?? "Проверяю…")}
        </span>
        <button className={button + " ml-auto"} onClick={() => query.refetch()}>
          Проверить
        </button>
      </div>
      {status?.reachable && (
        <label className="flex items-center gap-2">
          Модель
          <select
            value={status.model ?? ""}
            onChange={(e) => pick(e.target.value)}
            className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2 text-[13px] text-fg outline-none focus:border-accent/50"
          >
            <option value="">Первая из списка</option>
            {status.model && !status.models.includes(status.model) && <option value={status.model}>{status.model}</option>}
            {status.models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </label>
      )}
    </div>
  );
}
