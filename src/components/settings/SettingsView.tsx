import React, { useState } from "react";
import {
  Settings,
  Key,
  Shield,
  Download,
  CheckCircle2,
  AlertTriangle,
  ExternalLink,
  Trash2,
  Eye,
  EyeOff,
  Code2,
  HardDrive,
  RefreshCw,
} from "lucide-react";

import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";

export const SettingsView: React.FC = () => {
  const { token, isLoading: isAuthLoading, error: authError, saveToken, logout } =
    useAuthStore();
  const {
    status: binaryStatus,
    isDownloading,
    progress: downloadProgress,
    checkStatus,
    downloadManagedBinary,
  } = useBinaryStore();

  const [inputToken, setInputToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);

  const handleSaveToken = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!inputToken.trim()) return;
    const ok = await saveToken(inputToken.trim());
    if (ok) {
      setSaveSuccess(true);
      setInputToken("");
      setTimeout(() => setSaveSuccess(false), 3000);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6 max-w-4xl">
      {/* Header */}
      <div>
        <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
          <Settings className="w-5 h-5 text-blue-400" />
          Settings & Credentials
        </h1>
        <p className="text-xs text-zinc-400 mt-0.5">
          Manage your OS Keychain credentials, Cloudflare API tokens, and binary setup.
        </p>
      </div>

      {/* Cloudflare Authentication & Keychain Card */}
      <div className="p-6 rounded-2xl bg-zinc-900/50 border border-zinc-800 space-y-4">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2 font-bold text-sm text-zinc-100">
              <Key className="w-4 h-4 text-orange-400" />
              <span>Cloudflare API Token</span>
            </div>
            <p className="text-xs text-zinc-400">
              Encrypted inside your native operating system keychain (macOS Keychain / Windows Credential Manager).
            </p>
          </div>

          <a
            href="https://dash.cloudflare.com/profile/api-tokens"
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1.5 text-xs text-blue-400 hover:text-blue-300 transition cursor-pointer"
          >
            <span>Create Token on Cloudflare</span>
            <ExternalLink className="w-3.5 h-3.5" />
          </a>
        </div>

        {token ? (
          <div className="p-4 rounded-xl bg-zinc-950 border border-zinc-800 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-emerald-950/60 border border-emerald-800/60 flex items-center justify-center text-emerald-400">
                <Shield className="w-4 h-4" />
              </div>
              <div>
                <div className="text-xs font-semibold text-zinc-200">
                  Authenticated & Stored Securely
                </div>
                <div className="text-[11px] text-zinc-500 font-mono">
                  Token: ••••••••••••••••••••••••••••
                </div>
              </div>
            </div>

            <button
              onClick={logout}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-rose-950/40 hover:bg-rose-900/50 text-rose-300 border border-rose-800/60 text-xs font-medium transition cursor-pointer"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>Remove Token</span>
            </button>
          </div>
        ) : (
          <form onSubmit={handleSaveToken} className="space-y-3">
            <div className="relative">
              <input
                type={showToken ? "text" : "password"}
                required
                placeholder="Paste your scoped Cloudflare API token here..."
                value={inputToken}
                onChange={(e) => setInputToken(e.target.value)}
                className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-4 py-2.5 pr-10 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:outline-none font-mono"
              />
              <button
                type="button"
                onClick={() => setShowToken(!showToken)}
                className="absolute right-3 top-2.5 text-zinc-500 hover:text-zinc-300 transition"
              >
                {showToken ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>

            <div className="p-3 rounded-xl bg-zinc-950/60 border border-zinc-850 text-[11px] text-zinc-400 space-y-1">
              <div className="font-semibold text-zinc-300">Required Token Permissions:</div>
              <ul className="list-disc list-inside space-y-0.5 text-zinc-400">
                <li><code className="text-zinc-300 font-mono">Account.Cloudflare Tunnel:Edit</code></li>
                <li><code className="text-zinc-300 font-mono">Zone.DNS:Edit</code></li>
                <li><code className="text-zinc-300 font-mono">Account.Account Settings:Read</code> &amp; <code className="text-zinc-300 font-mono">Zone.Zone:Read</code></li>
              </ul>
            </div>

            <div className="flex items-center justify-between pt-1">
              {saveSuccess && (
                <div className="flex items-center gap-1.5 text-xs text-emerald-400">
                  <CheckCircle2 className="w-4 h-4" />
                  <span>Token verified and saved to OS Keychain!</span>
                </div>
              )}

              {authError && (
                <div className="text-xs text-rose-400 font-medium">
                  {authError}
                </div>
              )}

              <button
                type="submit"
                disabled={isAuthLoading || !inputToken.trim()}
                className="ml-auto px-5 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-50"
              >
                {isAuthLoading ? "Verifying Token..." : "Verify & Save into Keychain"}
              </button>
            </div>
          </form>
        )}
      </div>

      {/* Binary Management Card */}
      <div className="p-6 rounded-2xl bg-zinc-900/50 border border-zinc-800 space-y-4">
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2 font-bold text-sm text-zinc-100">
              <HardDrive className="w-4 h-4 text-purple-400" />
              <span>cloudflared Binary</span>
            </div>
            <p className="text-xs text-zinc-400">
              System detection and self-managed binary auto-updates.
            </p>
          </div>

          <button
            onClick={checkStatus}
            className="p-1.5 rounded-lg bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-zinc-200 transition cursor-pointer"
            title="Re-check binary"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>

        <div className="p-4 rounded-xl bg-zinc-950 border border-zinc-800 space-y-2.5">
          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-400">Status:</span>
            {binaryStatus?.is_installed ? (
              <span className="inline-flex items-center gap-1.5 text-emerald-400 font-medium">
                <CheckCircle2 className="w-3.5 h-3.5" />
                <span>Installed ({binaryStatus.is_managed ? "Managed by Teitunnel" : "System Binary"})</span>
              </span>
            ) : (
              <span className="inline-flex items-center gap-1.5 text-amber-400 font-medium">
                <AlertTriangle className="w-3.5 h-3.5" />
                <span>Not Found</span>
              </span>
            )}
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-400">Location:</span>
            <span className="font-mono text-zinc-200 text-[11px] truncate max-w-sm">
              {binaryStatus?.path || "None"}
            </span>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-400">Version:</span>
            <span className="font-mono text-zinc-300 text-[11px]">
              {binaryStatus?.version || "Unknown"}
            </span>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-400">Platform:</span>
            <span className="font-mono text-zinc-300 text-[11px]">
              {binaryStatus?.os} ({binaryStatus?.architecture})
            </span>
          </div>
        </div>

        {/* Download / Reinstall Button */}
        <div className="flex items-center justify-between pt-1">
          {downloadProgress && (
            <div className="text-xs text-zinc-400">
              {downloadProgress.status} ({downloadProgress.percentage?.toFixed(0)}%)
            </div>
          )}

          <button
            onClick={downloadManagedBinary}
            disabled={isDownloading}
            className="ml-auto flex items-center gap-2 px-4 py-2 rounded-xl bg-zinc-800 hover:bg-zinc-700 text-zinc-200 text-xs font-semibold border border-zinc-700 transition cursor-pointer disabled:opacity-50"
          >
            <Download className="w-3.5 h-3.5" />
            <span>
              {isDownloading
                ? "Downloading..."
                : binaryStatus?.is_installed
                ? "Reinstall / Update Managed Binary"
                : "Download Latest Official Binary"}
            </span>
          </button>
        </div>
      </div>

      {/* About & Open Source */}
      <div className="p-6 rounded-2xl bg-zinc-900/30 border border-zinc-850 flex items-center justify-between">
        <div className="space-y-1">
          <div className="font-bold text-sm text-zinc-200">Teitunnel v0.1.0</div>
          <p className="text-xs text-zinc-500">
            Open-source desktop client built by the teispace community. Licensed under MIT.
          </p>
        </div>

        <a
          href="https://github.com/teispace/teitunnel"
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-zinc-950 border border-zinc-800 text-xs text-zinc-300 hover:text-white transition cursor-pointer"
        >
          <Code2 className="w-4 h-4" />
          <span>GitHub Repository</span>
        </a>

      </div>
    </div>
  );
};
