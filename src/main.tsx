import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./lib/i18n";
import "./index.css";

// No React.StrictMode: its dev-only double-invoke of effects races Tauri's
// async listen()/unlisten() (see subscribe() in lib/ipc.ts) and was causing
// every chat event to be delivered twice in `tauri dev`. That double-invoke
// never happens in a production build anyway, so this only affects dev-mode
// fidelity, not the shipped app's correctness.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <I18nProvider>
    <App />
  </I18nProvider>,
);
