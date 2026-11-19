import { NavLink, Outlet } from "react-router-dom";
import {
  Activity,
  ArrowLeftRight,
  FolderOpen,
  LayoutDashboard,
  Server,
  Settings,
} from "lucide-react";
import mark from "@/assets/mark.svg";
import { InformationDialog } from "@/components/InformationDialog";
import { cn } from "@/lib/utils";

const NAV = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/mappings", label: "Mappings", icon: ArrowLeftRight, end: false },
  { to: "/traffic", label: "Traffic", icon: Activity, end: false },
  { to: "/services", label: "Services", icon: Server, end: false },
  { to: "/profiles", label: "Profiles", icon: FolderOpen, end: false },
  { to: "/settings", label: "Settings", icon: Settings, end: false },
];

export function AppShell() {
  return (
    <div className="flex h-full min-h-0 bg-background">
      <aside className="flex w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar">
        <div className="flex items-center gap-3 px-5 pb-4 pt-5">
          <img src={mark} alt="" className="size-9 shrink-0 rounded-[10px]" />
          <div>
            <div className="text-[11px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
              Loopback
            </div>
            <div className="mt-0.5 text-sm font-semibold leading-tight">Local Endpoint Router</div>
          </div>
        </div>
        <nav className="flex flex-1 flex-col gap-1 px-3">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors",
                  isActive
                    ? "bg-sidebar-accent text-sidebar-accent-foreground"
                    : "hover:bg-sidebar-accent/70 hover:text-foreground",
                )
              }
            >
              <item.icon className="size-4" />
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="border-t border-sidebar-border px-3 py-3">
          <InformationDialog />
          <p className="px-3 pt-3 text-xs leading-relaxed text-muted-foreground">
            Listener defaults to 127.0.0.1:8787. The browser follows the system proxy when that toggle is on.
          </p>
        </div>
      </aside>
      <main className="min-w-0 flex-1 overflow-auto">
        <div className="mx-auto w-full max-w-6xl px-8 py-7">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
