import { Outlet } from "react-router-dom";
import TitleBar from "./TitleBar";
import Sidebar from "./Sidebar";

export default function AppShell() {
  return (
    <div className="flex flex-col h-screen overflow-hidden">
      <TitleBar />
      <div className="flex flex-1 min-h-0">
        <Sidebar />
        <main className="flex-1 min-w-0 overflow-hidden bg-aonyx-50 dark:bg-aonyx-900">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
