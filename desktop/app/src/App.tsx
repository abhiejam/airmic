import "./theme.css";
import "./App.css";

import { Logo } from "./Logo";

export function App() {
  return (
    <main className="shell">
      <header className="brand">
        <Logo size={32} />
        <span className="wordmark">AirMic</span>
      </header>
      <section className="card">
        <h1>Connecting to the AirMic service</h1>
        <p>The status screen arrives with the IPC client.</p>
      </section>
    </main>
  );
}
