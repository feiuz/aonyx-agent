import { useEffect, useState } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "./context/ThemeContext";
import { AuthProvider } from "./context/AuthContext";
import { AgentProvider } from "./context/AgentContext";
import { LanguageProvider } from "./context/LanguageContext";
import { safeInvoke } from "./config/bridge";
import AppShell from "./layout/AppShell";
import Wizard from "./views/wizard/Wizard";
import Chat from "./views/Chat";
import SettingsHub from "./views/SettingsHub";

// First-run gate (ADR-016): render the onboarding wizard until setup is complete
// (a marker in ~/.aonyx/config.toml). Navigating to #/welcome forces it (preview
// / rerun). Outside Tauri (browser preview) we fall through to the app.
function SetupGate({ children }) {
  const [state, setState] = useState("loading");
  useEffect(() => {
    const force = window.location.hash.replace(/^#\/?/, "") === "welcome";
    (async () => {
      const s = await safeInvoke("setup_state", undefined, { configured: true });
      setState(!s?.configured || force ? "wizard" : "app");
    })();
  }, []);
  if (state === "loading") return <div className="h-screen bg-aonyx-50 dark:bg-aonyx-900" />;
  if (state === "wizard") return <Wizard onDone={() => setState("app")} />;
  return children;
}

// Hermes-style IA: the main sidebar shows only conversations; the chat is the
// landing (/), and every other surface lives under Settings (/settings).
// HashRouter: the active route lives in location.hash (works under tauri://).
export default function App() {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 1000 * 60 * 5,
            gcTime: 1000 * 60 * 10,
            refetchOnWindowFocus: false,
            retry: 1,
          },
        },
      }),
  );

  return (
    <QueryClientProvider client={queryClient}>
      <LanguageProvider>
        <ThemeProvider>
        <AuthProvider>
          <AgentProvider>
            <SetupGate>
            <HashRouter>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="/" element={<Chat />} />
                <Route path="/settings" element={<SettingsHub />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
            </HashRouter>
            </SetupGate>
          </AgentProvider>
        </AuthProvider>
        </ThemeProvider>
      </LanguageProvider>
    </QueryClientProvider>
  );
}
