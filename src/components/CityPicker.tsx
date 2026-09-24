import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, MapPin, Search } from "lucide-react";
import { usePrefs } from "../lib/prefs";
import { searchCities } from "../widgets/weather/api";

function useDebounced<T>(value: T, ms: number) {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return debounced;
}

export function CityPicker() {
  const { location, setLocation } = usePrefs();
  const [editing, setEditing] = useState(!location);
  const [text, setText] = useState("");
  const query = useDebounced(text.trim(), 350);

  const { data: results, isFetching, isError } = useQuery({
    queryKey: ["geocode", query],
    queryFn: () => searchCities(query),
    enabled: editing && query.length >= 2,
    staleTime: Infinity,
  });

  if (!editing && location) {
    return (
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <MapPin className="size-4 shrink-0 text-accent" />
          <div className="min-w-0">
            <div className="truncate text-[14px]">{location.name}</div>
            <div className="truncate text-[12px] text-fg-subtle">{location.detail}</div>
          </div>
        </div>
        <button
          onClick={() => setEditing(true)}
          className="shrink-0 rounded-md border border-stroke px-2.5 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg"
        >
          Изменить
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1 p-2">
      <label className="flex h-9 items-center gap-2 rounded-lg border border-stroke bg-field px-2.5 focus-within:border-accent/50">
        <Search className="size-4 text-fg-subtle" />
        <input
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Город, например Москва"
          spellCheck={false}
          className="h-full flex-1 bg-transparent text-[13px] outline-none placeholder:text-fg-subtle"
        />
      </label>
      {isError && <p className="px-1.5 py-1 text-[12px] text-warn">Не удалось найти, проверьте подключение</p>}
      {query.length >= 2 && !isFetching && results?.length === 0 && (
        <p className="px-1.5 py-1 text-[12px] text-fg-subtle">Ничего не найдено</p>
      )}
      {results?.map((city) => (
        <button
          key={`${city.latitude},${city.longitude}`}
          onClick={() => {
            setLocation(city);
            setEditing(false);
            setText("");
          }}
          className="flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-ink/8"
        >
          <MapPin className="size-3.5 shrink-0 text-fg-subtle" />
          <span className="min-w-0 flex-1 truncate text-[13px]">
            {city.name} <span className="text-fg-subtle">· {city.detail}</span>
          </span>
          {location?.latitude === city.latitude && location?.longitude === city.longitude && (
            <Check className="size-4 text-accent" />
          )}
        </button>
      ))}
      {location && (
        <button onClick={() => setEditing(false)} className="self-end px-2 py-1 text-[12px] text-fg-subtle hover:text-fg">
          Отмена
        </button>
      )}
    </div>
  );
}
