import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import { daemon, type Level, type Status } from "./ipc";

const SILENT: Level = { rms: 0, peak: 0 };

export type DaemonState = {
  connected: boolean;
  status: Status | null;
  level: Level;
};

/** Follows the daemon: connection, `status` and `level`. The Rust side reconnects and re-subscribes. */
export function useDaemon(): DaemonState {
  const [connected, setConnected] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);
  const [level, setLevel] = useState<Level>(SILENT);

  useEffect(() => {
    let active = true;
    const stops: Array<() => void> = [];

    // A `status` notification only comes when something changes, so fetch it on every connect.
    const fetchStatus = () =>
      daemon.status().then(
        (next) => {
          if (!active) return;
          setConnected(true);
          setStatus(next);
        },
        () => {},
      );

    (async () => {
      const unlisten = await Promise.all([
        listen<{ connected: boolean }>("daemon-connection", ({ payload }) => {
          if (payload.connected) {
            setConnected(true);
            void fetchStatus();
          } else {
            setConnected(false);
            setStatus(null);
            setLevel(SILENT);
          }
        }),
        listen<{ method: string; params: unknown }>("daemon-notification", ({ payload }) => {
          if (payload.method === "status") setStatus(payload.params as Status);
          if (payload.method === "level") setLevel(payload.params as Level);
        }),
      ]);
      // Listeners register after the first render, and the effect may already be torn down (StrictMode).
      if (active) stops.push(...unlisten);
      else unlisten.forEach((stop) => stop());
      void fetchStatus();
    })();

    return () => {
      active = false;
      stops.forEach((stop) => stop());
    };
  }, []);

  return { connected, status, level };
}
