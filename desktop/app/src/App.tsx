import "./theme.css";
import "./App.css";

import { Logo } from "./Logo";
import type { Status } from "./ipc";
import { useDaemon } from "./useDaemon";

function describe(connected: boolean, status: Status | null): { title: string; detail: string } {
  if (!connected || !status) {
    return { title: "Can't reach the AirMic service", detail: "It starts at login. Trying again every second." };
  }
  const name = status.phone?.name ?? "Your phone";
  switch (status.state) {
    case "streaming":
      return { title: `Streaming from ${name}`, detail: "Apps can use AirMic as a microphone." };
    case "muted":
      return { title: `${name} is muted`, detail: "Unmute on the phone to send audio." };
    case "idle":
      return { title: "Waiting for your phone", detail: "Open AirMic on the phone to connect." };
  }
}

export function App() {
  const { connected, status } = useDaemon();
  const { title, detail } = describe(connected, status);

  return (
    <main className="shell">
      <header className="brand">
        <Logo size={32} />
        <span className="wordmark">AirMic</span>
      </header>
      <section className="card" aria-live="polite">
        <h1>{title}</h1>
        <p>{detail}</p>
      </section>
    </main>
  );
}
