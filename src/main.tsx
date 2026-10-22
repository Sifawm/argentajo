import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { applyLanguage } from "./i18n";
import "./index.css";
import { loadPreferences } from "./preferences";

// Loaded first, so the language and the first Job's defaults are right from the start.
const preferences = await loadPreferences();
await applyLanguage(preferences.language);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App initialPreferences={preferences} />
  </StrictMode>,
);
