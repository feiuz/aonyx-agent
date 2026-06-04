import { useState } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "./context/ThemeContext";
import { AuthProvider } from "./context/AuthContext";
import { AgentProvider } from "./context/AgentContext";
import { LanguageProvider } from "./context/LanguageContext";
import AppShell from "./layout/AppShell";
import {
  Dashboard,
  Chat,
  Projets,
  Stats,
  MemoryHealth,
  KnowledgeGraph,
  Users,
  Permissions,
  Mcp,
  Settings,
} from "./views";

// HashRouter: the active route lives in location.hash, which works under the
// tauri:// origin (like Electron's file://) — BrowserRouter would 404 on reload.
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
            <HashRouter>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="/" element={<Dashboard />} />
                <Route path="/chat" element={<Chat />} />
                <Route path="/projects" element={<Projets />} />
                <Route path="/stats" element={<Stats />} />
                <Route path="/memory-health" element={<MemoryHealth />} />
                <Route path="/kg" element={<KnowledgeGraph />} />
                <Route path="/users" element={<Users />} />
                <Route path="/permissions" element={<Permissions />} />
                <Route path="/mcp" element={<Mcp />} />
                <Route path="/settings" element={<Settings />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
            </HashRouter>
          </AgentProvider>
        </AuthProvider>
        </ThemeProvider>
      </LanguageProvider>
    </QueryClientProvider>
  );
}
