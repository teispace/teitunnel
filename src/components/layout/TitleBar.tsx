import React, { useState, useEffect } from "react";
import {
  Globe,
  Radio,
  Cloud,
  Download,
  Shield,
  Settings,
  Maximize2,
  Minimize2,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";
import { useQuickTunnelStore } from "@/stores/quick-tunnel-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { SelectDropdown } from "@/components/ui/SelectDropdown";
import type { NavTab } from "@/components/layout/Sidebar";

interface TitleBarProps {
  currentTab?: NavTab;
  onTabChange?: (tab: NavTab) => void;
  onOpenSettings: () => void;
}

export const TitleBar: React.FC<TitleBarProps> = ({
  currentTab,
  onTabChange,
  onOpenSettings,
}) => {
  const {
    accounts,
    activeAccountId,
    selectAccount,
    zones,
    activeZoneId,
    selectZone,
    certStatus,
  } = useAuthStore();

  const { status, isDownloading, downloadManagedBinary } = useBinaryStore();
  const quickState = useQuickTunnelStore((s) => s.state);
  const activeProcesses = useTunnelStore((s) => s.activeProcesses);
  const runningTunnelsCount = Object.keys(activeProcesses).length;

  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    try {
      const win = getCurrentWindow();
      win.isMaximized().then(setIsMaximized).catch(() => {});
      win
        .onResized(() => {
          win.isMaximized().then(setIsMaximized).catch(() => {});
        })
        .then((un) => {
          unlisten = un;
        })
        .catch(() => {});
    } catch {
      // Ignored outside Tauri
    }

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const handleMouseDown = (e: React.MouseEvent<HTMLElement>) => {
    // Only primary button drag
    if (e.button !== 0) return;

    const target = e.target as HTMLElement;
    if (
      target.closest("button") ||
      target.closest("select") ||
      target.closest("input") ||
      target.closest("a") ||
      target.closest("[data-no-drag='true']")
    ) {
      return;
    }

    try {
      getCurrentWindow().startDragging().catch(() => {});
    } catch {
      // Ignored outside Tauri
    }
  };

  const handleDoubleClick = (e: React.MouseEvent<HTMLElement>) => {
    const target = e.target as HTMLElement;
    if (
      target.closest("button") ||
      target.closest("select") ||
      target.closest("input") ||
      target.closest("a") ||
      target.closest("[data-no-drag='true']")
    ) {
      return;
    }

    try {
      const win = getCurrentWindow();
      win
        .toggleMaximize()
        .then(async () => {
          const max = await win.isMaximized();
          setIsMaximized(max);
        })
        .catch((err) => {
          console.error("Failed to toggle maximize:", err);
        });
    } catch (err) {
      console.error("Failed to toggle maximize:", err);
    }
  };

  const handleToggleMaximize = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const win = getCurrentWindow();
      await win.toggleMaximize();
      const max = await win.isMaximized();
      setIsMaximized(max);
    } catch (err) {
      console.error("Failed to toggle maximize:", err);
    }
  };

  return (
    <header
      data-tauri-drag-region="true"
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
      className="h-11 bg-zinc-950/80 border-b border-white/[0.07] backdrop-blur-xl flex items-center justify-between px-3 select-none z-50 cursor-default shadow-[inset_0_1px_0_0_rgba(255,255,255,0.05)]"
    >
      {/* Left: macOS Traffic Lights offset & Brand */}
      <div
        data-tauri-drag-region="true"
        className="flex items-center gap-2 pl-18 sm:pl-20"
      >
        <div
          data-tauri-drag-region="true"
          className="flex items-center gap-2 group cursor-default"
        >
          <div className="w-5 h-5 rounded-md bg-blue-600/15 border border-blue-500/30 flex items-center justify-center text-blue-400 shadow-[0_0_10px_rgba(59,130,246,0.2)]">
            <Radio className="w-3 h-3 animate-pulse" />
          </div>
          <span className="font-semibold text-xs text-zinc-100 tracking-tight flex items-center gap-1.5">
            Teitunnel
            <span className="text-[10px] text-zinc-400 font-mono font-normal px-1.5 py-0.2 rounded bg-zinc-850/80 border border-zinc-750/60">
              v0.1.0
            </span>
          </span>
        </div>
      </div>

      {/* Middle: Native Dynamic Status & Scope Selector */}
      <div
        data-tauri-drag-region="true"
        className="flex-1 flex items-center justify-center gap-2 px-4"
      >
        {/* Active Tunnel Runtime Indicator (displayed alongside selectors) */}
        {quickState?.is_running ? (
          <button
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => onTabChange?.("quick-tunnel")}
            className="flex items-center gap-1.5 bg-amber-950/50 hover:bg-amber-900/60 border border-amber-600/40 px-2.5 py-0.5 rounded-full text-xs text-amber-300 transition cursor-pointer shadow-sm group shrink-0"
            title="Click to view Quick Tunnel session"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-ping" />
            <span className="font-mono font-medium text-[11px] truncate max-w-[130px]">
              Quick :{quickState.local_port}
            </span>
            <span className="text-[9px] uppercase font-bold text-amber-400/80 group-hover:text-amber-200">
              Live
            </span>
          </button>
        ) : runningTunnelsCount > 0 ? (
          <button
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => onTabChange?.("tunnels")}
            className="flex items-center gap-1.5 bg-emerald-950/50 hover:bg-emerald-900/60 border border-emerald-600/40 px-2.5 py-0.5 rounded-full text-xs text-emerald-300 transition cursor-pointer shadow-sm group shrink-0"
            title="Click to view running tunnels"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-ping" />
            <span className="text-[11px] font-medium">
              {runningTunnelsCount} Active
            </span>
          </button>
        ) : null}

        {/* Account and Zone Scope Selectors (Always visible when accounts exist) */}
        {accounts.length > 0 ? (
          <div
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            className="flex items-center gap-1.5"
          >
            <SelectDropdown
              options={accounts.map((acc) => ({
                value: acc.id,
                label: acc.name,
                sublabel: `ID: ${acc.id.slice(0, 8)}...`,
                icon: <Cloud className="w-3.5 h-3.5 text-orange-400" />,
              }))}
              value={activeAccountId || ""}
              onChange={(val) => selectAccount(val)}
              searchable={accounts.length > 3}
              placeholder="Select Account"
              triggerClassName="h-7 px-2.5 py-0 bg-zinc-900/90 border-zinc-800 hover:border-zinc-700 text-xs text-zinc-200 max-w-[160px]"
              menuClassName="min-w-[220px]"
            />

            {zones.length > 0 && (
              <>
                <span className="text-zinc-600 text-xs font-mono">/</span>
                <SelectDropdown
                  options={zones.map((zone) => ({
                    value: zone.id,
                    label: zone.name,
                    sublabel: zone.status,
                    badge: zone.status === "active" ? "Active" : zone.status,
                    badgeColor:
                      zone.status === "active"
                        ? "bg-emerald-950/60 text-emerald-300 border-emerald-800/40"
                        : "bg-zinc-800 text-zinc-400 border-zinc-700",
                    icon: <Globe className="w-3.5 h-3.5 text-blue-400" />,
                  }))}
                  value={activeZoneId || ""}
                  onChange={(val) => selectZone(val)}
                  searchable={zones.length > 3}
                  placeholder="Select Zone"
                  triggerClassName="h-7 px-2.5 py-0 bg-zinc-900/90 border-zinc-800 hover:border-zinc-700 text-xs text-zinc-200 max-w-[170px]"
                  menuClassName="min-w-[240px]"
                />
              </>
            )}
          </div>
        ) : certStatus?.has_cert ? (
          <div
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={onOpenSettings}
            className="flex items-center gap-1.5 bg-zinc-900/90 border border-emerald-800/50 hover:border-emerald-700/60 px-2.5 py-1 rounded-lg text-xs text-emerald-300 hover:bg-zinc-850 transition cursor-pointer shadow-sm"
            title="Logged in via Origin Certificate (~/.cloudflared/cert.pem)"
          >
            <Shield className="w-3.5 h-3.5 text-emerald-400" />
            <span className="font-medium">Origin Cert Active</span>
          </div>
        ) : (
          <button
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={onOpenSettings}
            className="flex items-center gap-1.5 text-xs text-zinc-400 hover:text-zinc-200 bg-zinc-900/60 hover:bg-zinc-800/80 border border-zinc-800/80 px-2.5 py-1 rounded-lg transition cursor-pointer"
          >
            <Cloud className="w-3.5 h-3.5 text-zinc-400" />
            <span>Connect Account / 1-Click Login</span>
          </button>
        )}
      </div>

      {/* Right: Status Pill & Window Actions */}
      <div
        data-tauri-drag-region="true"
        className="flex items-center gap-2"
      >
        {/* Binary Status Pill */}
        <div data-no-drag="true" onMouseDown={(e) => e.stopPropagation()}>
          {status?.is_installed ? (
            <div
              onClick={onOpenSettings}
              title={`cloudflared ready (${status.path || ""})`}
              className="flex items-center gap-1.5 text-xs text-emerald-400 bg-emerald-950/40 border border-emerald-800/40 px-2.5 py-0.5 rounded-full cursor-pointer hover:bg-emerald-900/40 transition"
            >
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400" />
              <span className="text-[11px] font-medium font-mono">
                {status.version?.split(" ")[2] || "ready"}
              </span>
            </div>
          ) : (
            <button
              onClick={downloadManagedBinary}
              disabled={isDownloading}
              className="flex items-center gap-1.5 text-xs text-amber-400 bg-amber-950/40 border border-amber-800/40 px-2.5 py-0.5 rounded-full cursor-pointer hover:bg-amber-900/40 transition disabled:opacity-60"
            >
              <Download className="w-3 h-3 animate-bounce" />
              <span className="text-[11px] font-medium">
                {isDownloading ? "Downloading..." : "Download cloudflared"}
              </span>
            </button>
          )}
        </div>

        {/* Quick Settings Icon */}
        <button
          data-no-drag="true"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={onOpenSettings}
          className={`p-1.5 rounded-lg border transition cursor-pointer ${
            currentTab === "settings"
              ? "bg-blue-600/20 border-blue-500/50 text-blue-400"
              : "bg-zinc-900 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 border-zinc-800"
          }`}
          title="Settings & Credentials"
        >
          <Settings className="w-3.5 h-3.5" />
        </button>

        {/* Window Expand / Collapse (Maximize / Restore) Icon Button */}
        <button
          data-no-drag="true"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={handleToggleMaximize}
          className="p-1.5 rounded-lg bg-zinc-900 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 border border-zinc-800 transition cursor-pointer"
          title={isMaximized ? "Restore Window (Collapse)" : "Maximize Window (Expand)"}
        >
          {isMaximized ? (
            <Minimize2 className="w-3.5 h-3.5 text-blue-400" />
          ) : (
            <Maximize2 className="w-3.5 h-3.5" />
          )}
        </button>
      </div>
    </header>
  );
};
