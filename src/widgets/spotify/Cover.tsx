import { useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { Music2 } from "lucide-react";

/**
 * A Spotify cover. If the web view can't load it from the CDN, the app
 * fetches it instead; failing that, a note icon stands in.
 */
export function Cover({ src, fallback }: { src: string | null; fallback?: ReactNode }) {
  const [direct, setDirect] = useState<"loading" | "ok" | "failed">("loading");
  const { data: proxied, isError } = useQuery({
    queryKey: ["spotify-image", src],
    queryFn: () => invoke<string>("spotify_image", { url: src }),
    enabled: !!src && direct === "failed",
    staleTime: Infinity,
    gcTime: 30 * 60_000,
    retry: false,
  });
  const url = direct === "failed" ? proxied : src;
  if (!src || (direct === "failed" && isError)) return fallback ?? <Music2 className="size-3.5 text-fg-subtle" />;
  if (!url) return null;
  return (
    <img
      src={url}
      className="size-full object-cover"
      draggable={false}
      loading="lazy"
      onLoad={() => direct === "loading" && setDirect("ok")}
      onError={() => direct !== "failed" && setDirect("failed")}
    />
  );
}
