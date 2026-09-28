import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { BatteryCharging, BatteryMedium, Cpu, HardDrive, MemoryStick, type LucideIcon } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";

interface SystemStats {
  cpu: number;
  memUsed: number;
  memTotal: number;
  diskUsed: number;
  diskTotal: number;
  battery: { percent: number; charging: boolean } | null;
}

const GB = 1024 ** 3;
const gb = (bytes: number) => (bytes / GB).toFixed(bytes < 100 * GB ? 1 : 0);

function Meter({ icon: Icon, label, percent, value }: { icon: LucideIcon; label: string; percent: number; value: string }) {
  const high = percent >= 85;
  return (
    <div className="flex items-center gap-2.5 text-[12px]">
      <Icon className="size-4 shrink-0 text-fg-muted" />
      <span className="w-14 shrink-0 text-fg-muted">{label}</span>
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-ink/10">
        <div
          className={clsx("h-full rounded-full transition-[width] duration-700", high ? "bg-amber-400" : "bg-accent")}
          style={{ width: `${Math.min(100, percent)}%` }}
        />
      </div>
      <span className="w-24 shrink-0 text-right tabular-nums text-fg-muted">{value}</span>
    </div>
  );
}

export function SystemWidget() {
  const { data } = useQuery({
    queryKey: ["system"],
    queryFn: () => invoke<SystemStats>("system_stats"),
    // Often enough for a glance; each reading redraws the widget.
    refetchInterval: 5000,
    staleTime: 0,
  });

  return (
    <Card title="Система" icon={Cpu}>
      {data ? (
        <div className="flex flex-col gap-2.5">
          <Meter icon={Cpu} label="ЦП" percent={data.cpu} value={`${Math.round(data.cpu)}%`} />
          <Meter
            icon={MemoryStick}
            label="Память"
            percent={(data.memUsed / data.memTotal) * 100}
            value={`${gb(data.memUsed)} / ${gb(data.memTotal)} ГБ`}
          />
          {data.diskTotal > 0 && (
            <Meter
              icon={HardDrive}
              label="Диск C:"
              percent={(data.diskUsed / data.diskTotal) * 100}
              value={`${gb(data.diskUsed)} / ${gb(data.diskTotal)} ГБ`}
            />
          )}
          {data.battery && (
            <Meter
              icon={data.battery.charging ? BatteryCharging : BatteryMedium}
              label="Батарея"
              percent={data.battery.percent}
              value={`${data.battery.percent}%${data.battery.charging ? " · заряд" : ""}`}
            />
          )}
        </div>
      ) : (
        <p className="text-[12px] text-fg-subtle">Загрузка…</p>
      )}
    </Card>
  );
}
