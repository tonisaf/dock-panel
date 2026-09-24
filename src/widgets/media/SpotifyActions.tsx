import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Heart } from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../../store";
import { setLiked, useSpotifyCurrent, useSpotifyStatus } from "../spotify/api";
import { DevicePicker } from "../spotify/DevicePicker";

/** Like and device buttons for the "now playing" card while Spotify is the player. */
export function SpotifyActions({ trackKey }: { trackKey: string }) {
  const queryClient = useQueryClient();
  const { data: status } = useSpotifyStatus();
  const connected = !!status?.connected && !status.error;
  const { data: track } = useSpotifyCurrent(trackKey, connected);
  const [error, setError] = useState<string | null>(null);
  if (!connected) return null;

  const toggleLike = async () => {
    if (!status.canLike) {
      usePanelStore.getState().setTab("settings");
      return;
    }
    if (!track) return;
    const liked = !track.liked;
    // Optimistic: the heart fills at once, and rolls back if Spotify refuses.
    queryClient.setQueryData(["spotify-current", trackKey], { ...track, liked });
    try {
      setError(null);
      await setLiked(track, liked);
      queryClient.invalidateQueries({ queryKey: ["spotify-library"] });
    } catch (e) {
      queryClient.setQueryData(["spotify-current", trackKey], track);
      setError(String(e));
    }
  };

  const title = !status.canLike
    ? "Чтобы ставить лайки, выйдите из Spotify в настройках и войдите снова"
    : (error ?? (track?.liked ? "Убрать из «Любимых треков»" : "Добавить в «Любимые треки»"));

  return (
    <div className="flex items-center">
      {(track || !status.canLike) && (
        <button
          onClick={toggleLike}
          title={title}
          className={clsx(
            "grid size-7 place-items-center rounded-full transition-colors hover:bg-ink/10",
            track?.liked ? "text-[#1ed760]" : "text-fg-muted hover:text-fg",
            (error || !status.canLike) && "text-warn",
          )}
        >
          <Heart className="size-4" fill={track?.liked ? "currentColor" : "none"} />
        </button>
      )}
      <DevicePicker />
    </div>
  );
}
