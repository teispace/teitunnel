import React, { useState } from "react";
import {
  Zap,
  Globe,
  Copy,
  ExternalLink,
  QrCode,
  Square,
  ArrowRight,
  ShieldAlert,
  Terminal as TerminalIcon,
  Check,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { useQuickTunnelStore } from "@/stores/quick-tunnel-store";
import { useBinaryStore } from "@/stores/binary-store";

export const QuickTunnelView: React.FC = () => {
  const { state, localPort, isStarting, error, setPort, start, stop } = useQuickTunnelStore();
  const binaryStatus = useBinaryStore((s) => s.status);
  const [showQr, setShowQr] = useState(false);
  const [copied, setCopied] = useState(false);

  const presets = [
    { label: "Next / React", port: 3000 },
    { label: "Vite Dev", port: 5173 },
    { label: "API / Go / Rust", port: 8080 },
    { label: "n8n Automation", port: 5678 },
    { label: "FastAPI / Python", port: 8000 },
  ];

  const handleCopy = () => {
    if (state?.public_url) {
      navigator.clipboard.writeText(state.public_url);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleOpenBrowser = () => {
    if (state?.public_url) {
      window.open(state.public_url, "_blank");
    }
  };

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header Banner */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <Zap className="w-5 h-5 text-amber-400" />
            Try Instantly (Quick Tunnel)
          </h1>
          <p className="text-xs text-zinc-400 mt-1">
            Zero-config port forwarding powered by Cloudflare edge. No login or domain required.
          </p>
        </div>

        {state?.is_running && (
          <div className="flex items-center gap-2">
            <div className="w-2.5 h-2.5 rounded-full bg-emerald-500 animate-ping" />
            <span className="text-xs font-semibold text-emerald-400">Live & Forwarding</span>
          </div>
        )}
      </div>

      {!binaryStatus?.is_installed && (
        <div className="p-4 rounded-xl bg-amber-950/30 border border-amber-800/50 flex items-start gap-3">
          <ShieldAlert className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />
          <div className="text-xs text-amber-200">
            <div className="font-semibold">cloudflared binary missing</div>
            <div className="text-amber-300/80 mt-0.5">
              Click &apos;Download cloudflared&apos; in the top bar to automatically install the managed binary.
            </div>
          </div>
        </div>
      )}

      {/* Main Controller Card */}
      <div className="p-6 rounded-2xl bg-zinc-900/60 border border-zinc-800/80 backdrop-blur-sm space-y-6">
        <div className="space-y-3">
          <label className="text-xs font-semibold text-zinc-300 uppercase tracking-wider">
            1. Select or Enter Local Port
          </label>

          {/* Quick Presets */}
          <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
            {presets.map((preset) => (
              <button
                key={preset.port}
                disabled={state?.is_running}
                onClick={() => setPort(preset.port)}
                className={`px-3 py-2 rounded-lg text-xs font-medium text-left border transition cursor-pointer ${
                  localPort === preset.port
                    ? "bg-blue-600/20 border-blue-500 text-blue-300"
                    : "bg-zinc-950/60 border-zinc-800 text-zinc-400 hover:border-zinc-700 hover:text-zinc-200"
                } ${state?.is_running ? "opacity-50 cursor-not-allowed" : ""}`}
              >
                <div className="font-mono text-zinc-200 font-bold">{preset.port}</div>
                <div className="text-[10px] text-zinc-400 truncate">{preset.label}</div>
              </button>
            ))}
          </div>

          {/* Custom Port Input */}
          <div className="flex items-center gap-3 pt-2">
            <span className="text-xs text-zinc-400 font-medium">Custom Port:</span>
            <input
              type="number"
              disabled={state?.is_running}
              value={localPort}
              onChange={(e) => setPort(parseInt(e.target.value) || 3000)}
              className="w-32 bg-zinc-950 border border-zinc-700/80 rounded-lg px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-blue-500 focus:outline-none"
              placeholder="3000"
            />
            <span className="text-xs text-zinc-500 font-mono">http://localhost:{localPort}</span>
          </div>
        </div>

        {/* Start / Stop Actions */}
        <div className="pt-2 border-t border-zinc-800 flex items-center justify-between">
          <div className="text-xs text-zinc-400">
            {state?.is_running ? (
              <span>Process PID: <strong className="text-zinc-200 font-mono">{state.pid}</strong></span>
            ) : (
              <span>Ready to establish secure QUIC tunnel</span>
            )}
          </div>

          {state?.is_running ? (
            <button
              onClick={stop}
              className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-rose-600 hover:bg-rose-500 text-white text-xs font-semibold shadow-lg shadow-rose-950/40 transition cursor-pointer"
            >
              <Square className="w-3.5 h-3.5 fill-current" />
              Stop Quick Tunnel
            </button>
          ) : (
            <button
              onClick={() => start(localPort)}
              disabled={isStarting}
              className="flex items-center gap-2 px-6 py-2.5 rounded-xl bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 text-white text-xs font-semibold shadow-lg shadow-blue-950/50 transition cursor-pointer disabled:opacity-60"
            >
              <Zap className="w-4 h-4 text-amber-300" />
              {isStarting ? "Establishing Tunnel..." : "Start Instant Tunnel"}
              <ArrowRight className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Active Public URL Display Card */}
      {state?.is_running && (
        <div className="p-6 rounded-2xl bg-gradient-to-br from-emerald-950/40 via-zinc-900/60 to-zinc-950 border border-emerald-500/40 shadow-xl space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-xs font-semibold text-emerald-400 uppercase tracking-wider">
              <Globe className="w-4 h-4" />
              Public Edge Endpoint Active
            </div>
            <button
              onClick={() => setShowQr(!showQr)}
              className="flex items-center gap-1.5 text-xs text-zinc-300 hover:text-white bg-zinc-800/80 hover:bg-zinc-700/80 px-2.5 py-1 rounded-lg border border-zinc-700/60 transition cursor-pointer"
            >
              <QrCode className="w-3.5 h-3.5" />
              <span>{showQr ? "Hide QR" : "Show Mobile QR"}</span>
            </button>
          </div>

          <div className="flex flex-col sm:flex-row items-center justify-between gap-3 p-3.5 rounded-xl bg-zinc-950/80 border border-zinc-800">
            <div className="font-mono text-sm text-emerald-300 select-all truncate">
              {state.public_url || "Waiting for edge assignment..."}
            </div>

            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={handleCopy}
                disabled={!state.public_url}
                className="flex items-center gap-1.5 text-xs bg-zinc-800 hover:bg-zinc-700 text-zinc-200 px-3 py-1.5 rounded-lg border border-zinc-700 transition cursor-pointer"
              >
                {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                <span>{copied ? "Copied!" : "Copy Link"}</span>
              </button>

              <button
                onClick={handleOpenBrowser}
                disabled={!state.public_url}
                className="flex items-center gap-1.5 text-xs bg-blue-600 hover:bg-blue-500 text-white px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                <span>Open in Browser</span>
              </button>
            </div>
          </div>

          {/* QR Code Expansion */}
          {showQr && state.public_url && (
            <div className="p-4 rounded-xl bg-zinc-950 border border-zinc-800 flex flex-col items-center justify-center space-y-2">
              <div className="p-3 bg-white rounded-xl shadow-lg">
                <QRCodeSVG value={state.public_url} size={160} />
              </div>
              <p className="text-[11px] text-zinc-400">
                Scan with your phone to open this tunnel on mobile data or any outside network!
              </p>
            </div>
          )}
        </div>
      )}

      {/* Error display */}
      {error && (
        <div className="p-4 rounded-xl bg-rose-950/40 border border-rose-800/60 text-xs text-rose-300">
          <strong>Error:</strong> {error}
        </div>
      )}

      {/* Live Stream Terminal Preview */}
      <div className="p-5 rounded-2xl bg-zinc-950 border border-zinc-800/80 space-y-3">
        <div className="flex items-center justify-between text-xs text-zinc-400">
          <div className="flex items-center gap-2 font-medium">
            <TerminalIcon className="w-4 h-4 text-zinc-500" />
            <span>Process Logs</span>
          </div>
          <span className="font-mono text-[11px] text-zinc-500">
            {state?.logs.length || 0} lines buffered
          </span>
        </div>

        <div className="h-44 bg-zinc-900/60 border border-zinc-850 rounded-xl p-3 font-mono text-[11px] text-zinc-300 overflow-y-auto space-y-1">
          {state?.logs && state.logs.length > 0 ? (
            state.logs.map((line, idx) => (
              <div key={idx} className="whitespace-pre-wrap leading-relaxed break-all">
                {line}
              </div>
            ))
          ) : (
            <div className="text-zinc-600 italic">No output yet. Start the tunnel to see live stdout/stderr.</div>
          )}
        </div>
      </div>
    </div>
  );
};
