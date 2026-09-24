import { useState } from "react";
import { AppWindow } from "lucide-react";
import { iconUrl } from "../lib/apps";

export function AppIcon({ id, size = 40 }: { id: string; size?: number }) {
  const [failed, setFailed] = useState(false);

  if (failed) {
    return (
      <div className="grid place-items-center rounded-xl bg-ink/8" style={{ width: size, height: size }}>
        <AppWindow className="text-fg-subtle" style={{ width: size * 0.5, height: size * 0.5 }} />
      </div>
    );
  }
  return (
    <img
      src={iconUrl(id)}
      width={size}
      height={size}
      loading="lazy"
      decoding="async"
      draggable={false}
      onError={() => setFailed(true)}
      className="object-contain"
      style={{ width: size, height: size }}
    />
  );
}
