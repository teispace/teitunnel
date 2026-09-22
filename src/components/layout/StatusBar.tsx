import React from "react";
import {
  Shield,
  Activity,
  Terminal,
  Radio,
  ExternalLink,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";
import { useQuickTunnelStore } from "@/stores/quick-tunnel-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { useLogStore } from "@/stores/log-store";
import type { NavTab } from "@/components/layout/Sidebar";

interface StatusBarProps {
  currentTab: NavTab;
  onTabChange: (tab: NavTab) => void;
}

export const StatusBar: React.FC<StatusBarProps> = ({ currentTab, onTabChange }) => {
  const { certStatus, token } = useAuthStore();
  const { status: binaryStatus } = useBinaryStore();
  const { state: quickState } = useQuickTunnelStore();
  const { activeProcesses } = useTunnelStore();
  const { logs } = useLogStore();

  const activeProcessCount = Object.keys(activeProcesses).length;
  const isQuickRunning = Boolean(quickState?.is_running);

  const handleMouseDown = (e: React.MouseEvent<HTMLElement>) => {
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
      getCurrentWindow().toggleMaximize().catch((err) => {
        console.error("Failed to toggle maximize:", err);
      });
    } catch (err) {
      console.error("Failed to toggle maximize:", err);
    }
  };

  return (
    <footer
      data-tauri-drag-region="true"
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
      className="h-6 bg-zinc-950/90 border-t border-white/[0.06] px-3 flex items-center justify-between text-[11px] text-zinc-400 select-none z-40 cursor-default"
    >
      {/* Left: System & Binary */}
      <div data-tauri-drag-region="true" className="flex items-center gap-3">
        <div className="flex items-center gap-1.5 font-mono">
          <span
            className={`w-1.5 h-1.5 rounded-full ${
              binaryStatus?.is_installed
                ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.6)]"
                : "bg-amber-400"
            }`}
          />
          <span className="text-zinc-300">
            {binaryStatus?.is_installed
              ? binaryStatus.version?.split(" ")[2] || "cloudflared"
              : "binary missing"}
          </span>
          {binaryStatus?.architecture && (
            <span className="text-zinc-600 hidden sm:inline">
              ({binaryStatus.os}-{binaryStatus.architecture})
            </span>
          )}
        </div>

        <span className="text-zinc-700 hidden sm:inline">•</span>

        <div className="hidden sm:flex items-center gap-1 text-zinc-400">
          <Shield className="w-3 h-3 text-emerald-400" />
          <span>
            {certStatus?.has_cert
              ? "Origin Cert"
              : token
              ? "Keychain Encrypted"
              : "No Credentials"}
          </span>
        </div>
      </div>

      {/* Center: Live Tunnel Status */}
      <div data-tauri-drag-region="true" className="flex items-center gap-2">
        {isQuickRunning ? (
          <button
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => onTabChange("quick-tunnel")}
            className="flex items-center gap-1.5 text-amber-400 hover:text-amber-300 transition cursor-pointer font-mono"
          >
            <Radio className="w-3 h-3 animate-pulse text-amber-400" />
            <span className="truncate max-w-[240px]">
              {quickState?.public_url
                ? quickState.public_url.replace("https://", "")
                : `Forwarding :${quickState?.local_port}`}
            </span>
            <ExternalLink className="w-2.5 h-2.5 opacity-60" />
          </button>
        ) : activeProcessCount > 0 ? (
          <button
            data-no-drag="true"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => onTabChange("tunnels")}
            className="flex items-center gap-1.5 text-emerald-400 hover:text-emerald-300 transition cursor-pointer font-mono"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-ping" />
            <span>
              {activeProcessCount} Zero Trust Tunnel{activeProcessCount > 1 ? "s" : ""}{" "}
              Active
            </span>
          </button>
        ) : (
          <span className="text-zinc-500 font-mono hidden md:inline">
            Idle • Ready for Tunnels
          </span>
        )}
      </div>

      {/* Right: Quick Tools & Diagnostics */}
      <div data-tauri-drag-region="true" className="flex items-center gap-3">
        <button
          data-no-drag="true"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => onTabChange("terminal")}
          className={`flex items-center gap-1.5 hover:text-zinc-200 transition cursor-pointer ${
            currentTab === "terminal" ? "text-blue-400 font-medium" : "text-zinc-500"
          }`}
          title="Open Terminal & Logs"
        >
          <Terminal className="w-3 h-3" />
          <span className="font-mono">{logs.length} logs</span>
        </button>

        <span className="text-zinc-700 hidden sm:inline">•</span>

        <button
          data-no-drag="true"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => onTabChange("telemetry")}
          className={`flex items-center gap-1 hover:text-zinc-200 transition cursor-pointer ${
            currentTab === "telemetry" ? "text-purple-400 font-medium" : "text-zinc-500"
          }`}
          title="View Prometheus Telemetry"
        >
          <Activity className="w-3 h-3" />
          <span className="hidden sm:inline">Telemetry</span>
        </button>
      </div>
    </footer>
  );
};
