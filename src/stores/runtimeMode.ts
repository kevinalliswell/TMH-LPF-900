import { useEffect, useState } from "react";
import { getRuntimeMode, type RuntimeMode } from "../ipc";

// The runtime mode never changes during a session, so fetch it once and cache it.
let cached: RuntimeMode | null = null;

/** Runtime mode fetched once from the backend and cached; `null` until it resolves. */
export function useRuntimeMode(): RuntimeMode | null {
  const [mode, setMode] = useState<RuntimeMode | null>(cached);
  useEffect(() => {
    if (cached !== null) {
      setMode(cached);
      return;
    }
    let alive = true;
    void getRuntimeMode().then((m) => {
      cached = m;
      if (alive) setMode(m);
    });
    return () => {
      alive = false;
    };
  }, []);
  return mode;
}
