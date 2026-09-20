import React from "react";
import {
  Zap,
  Network,
  GitBranch,
  ShieldCheck,
  Activity,
  Terminal,
  Settings,
  Sparkles,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useQuickTunnelStore } from "@/stores/quick-tunnel-store";
import { useTunnelStore } from "@/stores/tunnel-store";

export type NavTab =
  | "quick-tunnel"
  | "tunnels"
  | "ingress"
  | "dns"
  | "telemetry"
  | "terminal"
  | "settings";

interface SidebarProps {
  currentTab: NavTab;
  onTabChange: (tab: NavTab) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({ currentTab, onTabChange }) => {
  const quickTunnelRunning = useQuickTunnelStore((s) => s.state?.is_running);
  const activeProcesses = useTunnelStore((s) => s.activeProcesses);
  const runningTunnelsCount = Object.keys(activeProcesses).length;

  const navItems = [
    {
      id: "quick-tunnel" as NavTab,
      label: "Try Instantly",
      subtitle: "1-Click Ephemeral Tunnel",
      icon: Zap,
      badge: quickTunnelRunning ? "Active" : undefined,
      badgeColor: "bg-emerald-500/20 text-emerald-400 border-emerald-500/30",
    },
    {
      id: "tunnels" as NavTab,
      label: "Zero Trust Tunnels",
      subtitle: "Remotely-Managed",
      icon: Network,
      badge: runningTunnelsCount > 0 ? `${runningTunnelsCount} Live` : undefined,
      badgeColor: "bg-blue-500/20 text-blue-400 border-blue-500/30",
    },
    {
      id: "ingress" as NavTab,
      label: "Ingress Routing",
      subtitle: "Visual Rule Builder",
      icon: GitBranch,
    },
    {
      id: "dns" as NavTab,
      label: "DNS & Hygiene",
      subtitle: "No-Mess CNAME Manager",
      icon: ShieldCheck,
    },
    {
      id: "telemetry" as NavTab,
      label: "Telemetry & Colos",
      subtitle: "Edge Health & Metrics",
      icon: Activity,
    },
    {
      id: "terminal" as NavTab,
      label: "Terminal & Logs",
      subtitle: "Stream & CLI Runner",
      icon: Terminal,
    },
    {
      id: "settings" as NavTab,
      label: "Settings & Auth",
      subtitle: "Keychain & Binary",
      icon: Settings,
    },
  ];

  return (
    <aside className="w-64 bg-zinc-950 border-r border-zinc-850 flex flex-col justify-between select-none">
      <div className="p-3 space-y-1">
        <div className="px-3 py-2 text-[11px] font-medium text-zinc-500 uppercase tracking-wider">
          Management
        </div>

        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = currentTab === item.id;
          return (
            <button
              key={item.id}
              onClick={() => onTabChange(item.id)}
              className={cn(
                "w-full flex items-center justify-between px-3 py-2.5 rounded-lg text-left transition-all group cursor-pointer",
                isActive
                  ? "bg-zinc-800/90 text-zinc-100 shadow-sm border border-zinc-700/60"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/60"
              )}
            >
              <div className="flex items-center gap-3">
                <Icon
                  className={cn(
                    "w-4 h-4 transition-colors",
                    isActive ? "text-blue-400" : "text-zinc-400 group-hover:text-zinc-300"
                  )}
                />
                <div>
                  <div className="text-xs font-medium leading-none">{item.label}</div>
                  <div className="text-[10px] text-zinc-500 mt-0.5 leading-none">
                    {item.subtitle}
                  </div>
                </div>
              </div>

              {item.badge && (
                <span
                  className={cn(
                    "text-[10px] px-1.5 py-0.5 rounded font-mono border",
                    item.badgeColor
                  )}
                >
                  {item.badge}
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/* Bottom Promo / Open Source status */}
      <div className="p-3 border-t border-zinc-900">
        <div className="p-2.5 rounded-lg bg-gradient-to-br from-blue-950/30 via-zinc-900/40 to-purple-950/20 border border-zinc-800/80">
          <div className="flex items-center gap-2 text-xs font-medium text-zinc-200">
            <Sparkles className="w-3.5 h-3.5 text-blue-400" />
            <span>Open Source Swiss Knife</span>
          </div>
          <p className="text-[11px] text-zinc-400 mt-1 leading-relaxed">
            Fast, zero-telemetry client for Cloudflare Tunnels built by teispace.
          </p>
        </div>
      </div>
    </aside>
  );
};
