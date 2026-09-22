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
  Globe,
  Zap,
  Play,
  Square,
  Sparkles,
  Loader2,
  Plus,
} from "lucide-react";

import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { ConfirmModal } from "@/components/ui/ConfirmModal";
import { toast } from "sonner";

export const SettingsView: React.FC = () => {
  const {
    token,
    certStatus,
    isLoggingInBrowser,
    browserLoginUrl,
    isLoading: isAuthLoading,
    error: authError,
    startBrowserLogin,
    cancelBrowserLogin,
    logoutCert,
    saveToken,
    logout,
  } = useAuthStore();

  const {
    directTunnels,
    activeProcesses,
    saveDirectTunnel,
    deleteDirectTunnel,
    startDirectTunnel,
    stopTunnel,
  } = useTunnelStore();

  const {
    status: binaryStatus,
    isDownloading,
    progress: downloadProgress,
    checkStatus,
    downloadManagedBinary,
  } = useBinaryStore();

  const [authTab, setAuthTab] = useState<"browser" | "token" | "api">("browser");

  // API Token form state
  const [inputToken, setInputToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);

  // Direct Token form state
  const [directName, setDirectName] = useState("");
  const [directTokenInput, setDirectTokenInput] = useState("");
  const [showDirectToken, setShowDirectToken] = useState(false);
  const [directAddSuccess, setDirectAddSuccess] = useState(false);

  // Confirmation dialog states
  const [showConfirmCertLogout, setShowConfirmCertLogout] = useState(false);
  const [showConfirmApiLogout, setShowConfirmApiLogout] = useState(false);
  const [directTunnelToDelete, setDirectTunnelToDelete] = useState<{ id: string; name: string } | null>(null);

  const handleSaveToken = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!inputToken.trim()) return;
    toast.info("Verifying Cloudflare API token...");
    const ok = await saveToken(inputToken.trim());
    if (ok) {
      setSaveSuccess(true);
      setInputToken("");
      toast.success("API token verified and saved into OS Keychain!");
      setTimeout(() => setSaveSuccess(false), 3000);
    } else {
      toast.error("Invalid token. Please check permissions.");
    }
  };

  const handleAddDirectTunnel = (e: React.FormEvent) => {
    e.preventDefault();
    if (!directTokenInput.trim()) return;
    const name = directName.trim() || "Zero Trust Tunnel";
    saveDirectTunnel(name, directTokenInput.trim());
    toast.success(`Saved tunnel token for "${name}"`);
    setDirectName("");
    setDirectTokenInput("");
    setDirectAddSuccess(true);
    setTimeout(() => setDirectAddSuccess(false), 3000);
  };

  const handleLogoutCert = async () => {
    await logoutCert();
    toast.info("Deleted Origin Certificate (~/.cloudflared/cert.pem)");
  };

  const handleLogoutApi = async () => {
    await logout();
    toast.info("Removed API token from OS Keychain");
  };

  const handleStartDirect = async (id: string, name: string, tok: string) => {
    await startDirectTunnel(id, tok);
    toast.success(`Started tunnel "${name}"`);
  };

  const handleStopDirect = async (id: string, name: string) => {
    await stopTunnel(id);
    toast.info(`Stopped tunnel "${name}"`);
  };

  const handleDeleteDirect = (id: string, name: string) => {
    deleteDirectTunnel(id);
    toast.info(`Deleted saved tunnel "${name}"`);
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
          Choose between easy 1-click browser login, direct Zero Trust tokens, or scoped API tokens.
        </p>
      </div>

      {/* Auth Method Selector */}
      <div className="p-6 rounded-2xl bg-zinc-900/50 border border-zinc-800 space-y-5">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-zinc-800/80 pb-4">
          <div>
            <div className="font-bold text-sm text-zinc-100 flex items-center gap-2">
              <Shield className="w-4 h-4 text-emerald-400" />
              <span>Authentication &amp; Tunnel Setup</span>
            </div>
            <p className="text-xs text-zinc-400 mt-0.5">
              Select how you want Teitunnel to connect with Cloudflare.
            </p>
          </div>

          {/* Mode Switcher */}
          <div className="flex items-center gap-1 bg-zinc-950 p-1 rounded-xl border border-zinc-800 text-xs">
            <button
              onClick={() => setAuthTab("browser")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                authTab === "browser"
                  ? "bg-blue-600 text-white shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <Globe className="w-3.5 h-3.5" />
              <span>1-Click Browser</span>
              <span className="text-[10px] uppercase font-bold tracking-wider px-1 rounded bg-blue-500/30 text-blue-200">
                Easy
              </span>
            </button>

            <button
              onClick={() => setAuthTab("token")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                authTab === "token"
                  ? "bg-blue-600 text-white shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <Zap className="w-3.5 h-3.5" />
              <span>Tunnel Token</span>
            </button>

            <button
              onClick={() => setAuthTab("api")}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                authTab === "api"
                  ? "bg-blue-600 text-white shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <Key className="w-3.5 h-3.5" />
              <span>API Token</span>
            </button>
          </div>
        </div>

        {/* TAB 1: 1-Click Browser Login */}
        {authTab === "browser" && (
          <div className="space-y-4">
            <div className="space-y-1">
              <div className="text-xs font-semibold text-zinc-200 flex items-center gap-1.5">
                <Sparkles className="w-3.5 h-3.5 text-amber-400" />
                <span>Zero-Token Authentication via Origin Certificate</span>
              </div>
              <p className="text-xs text-zinc-400">
                Log in via Cloudflare in your browser to authorize your domains. A local certificate (
                <code className="text-zinc-300 font-mono">cert.pem</code>) is saved on your system, allowing Teitunnel to create, list, and run named tunnels with zero API token hassle.
              </p>
            </div>

            {certStatus?.has_cert ? (
              <div className="p-4 rounded-xl bg-emerald-950/30 border border-emerald-800/50 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                  <div className="w-9 h-9 rounded-xl bg-emerald-900/50 border border-emerald-700/60 flex items-center justify-center text-emerald-400">
                    <CheckCircle2 className="w-5 h-5" />
                  </div>
                  <div>
                    <div className="text-xs font-semibold text-emerald-300">
                      Origin Certificate Active (Logged In)
                    </div>
                    <div className="text-[11px] text-zinc-400 font-mono mt-0.5 truncate max-w-md">
                      {certStatus.cert_path || "~/.cloudflared/cert.pem"}
                    </div>
                  </div>
                </div>

                <button
                  onClick={() => setShowConfirmCertLogout(true)}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-rose-950/40 hover:bg-rose-900/50 text-rose-300 border border-rose-800/60 text-xs font-medium transition cursor-pointer self-start sm:self-auto"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                  <span>Log Out (Delete Cert)</span>
                </button>
              </div>
            ) : isLoggingInBrowser ? (
              <div className="p-5 rounded-xl bg-zinc-950 border border-blue-500/50 space-y-3 animate-pulse">
                <div className="flex items-center gap-3 text-xs text-blue-300 font-medium">
                  <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                  <span>Waiting for Cloudflare authorization in your browser...</span>
                </div>
                <p className="text-[11px] text-zinc-400">
                  Select your zone/domain on Cloudflare to authorize Teitunnel. Once authorized, Teitunnel will download the certificate automatically.
                </p>
                <div className="flex items-center gap-3 pt-1">
                  {browserLoginUrl && (
                    <a
                      href={browserLoginUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition cursor-pointer"
                    >
                      <span>Open Browser Authorization</span>
                      <ExternalLink className="w-3.5 h-3.5" />
                    </a>
                  )}
                  <button
                    onClick={cancelBrowserLogin}
                    className="px-3 py-1.5 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-zinc-300 text-xs transition cursor-pointer"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <div className="p-4 rounded-xl bg-zinc-950/70 border border-zinc-800 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
                <div className="space-y-0.5">
                  <div className="text-xs font-semibold text-zinc-200">
                    No certificate found on this computer
                  </div>
                  <div className="text-[11px] text-zinc-400">
                    Clicking below will open the official Cloudflare login page in your default browser.
                  </div>
                </div>

                <button
                  onClick={startBrowserLogin}
                  disabled={!binaryStatus?.is_installed}
                  className="flex items-center gap-2 px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-50"
                >
                  <Globe className="w-3.5 h-3.5" />
                  <span>Log in with Cloudflare (Browser)</span>
                </button>
              </div>
            )}
          </div>
        )}

        {/* TAB 2: Direct Tunnel Token */}
        {authTab === "token" && (
          <div className="space-y-4">
            <div className="space-y-1">
              <div className="text-xs font-semibold text-zinc-200 flex items-center gap-1.5">
                <Zap className="w-3.5 h-3.5 text-amber-400" />
                <span>Zero Trust Tunnel Token Runner</span>
              </div>
              <p className="text-xs text-zinc-400">
                Created a tunnel in Cloudflare Zero Trust dashboard? Paste its connector token (
                <code className="text-zinc-300 font-mono">eyJh...</code>) here to run and supervise it immediately without configuring any API keys.
              </p>
            </div>

            <form onSubmit={handleAddDirectTunnel} className="space-y-3 p-4 rounded-xl bg-zinc-950 border border-zinc-850">
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="space-y-1 sm:col-span-1">
                  <label className="text-[11px] font-medium text-zinc-400">Tunnel Name / Label</label>
                  <input
                    type="text"
                    placeholder="e.g. Home Server"
                    value={directName}
                    onChange={(e) => setDirectName(e.target.value)}
                    className="w-full bg-zinc-900 border border-zinc-750 rounded-lg px-3 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:outline-none"
                  />
                </div>

                <div className="space-y-1 sm:col-span-2">
                  <label className="text-[11px] font-medium text-zinc-400">Tunnel Connector Token</label>
                  <div className="relative">
                    <input
                      type={showDirectToken ? "text" : "password"}
                      required
                      placeholder="eyJh..."
                      value={directTokenInput}
                      onChange={(e) => setDirectTokenInput(e.target.value)}
                      className="w-full bg-zinc-900 border border-zinc-750 rounded-lg px-3 py-2 pr-9 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:outline-none font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => setShowDirectToken(!showDirectToken)}
                      className="absolute right-2.5 top-2 text-zinc-500 hover:text-zinc-300 transition cursor-pointer"
                    >
                      {showDirectToken ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                    </button>
                  </div>
                </div>
              </div>

              <div className="flex items-center justify-between pt-1">
                {directAddSuccess ? (
                  <div className="flex items-center gap-1.5 text-xs text-emerald-400">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    <span>Tunnel token saved! You can run it below or from the Tunnels tab.</span>
                  </div>
                ) : (
                  <div className="text-[11px] text-zinc-500">
                    Tokens are saved locally and can be toggled on/off with 1 click.
                  </div>
                )}

                <button
                  type="submit"
                  disabled={!directTokenInput.trim()}
                  className="flex items-center gap-1.5 px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition cursor-pointer disabled:opacity-50 ml-auto"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span>Save Tunnel Token</span>
                </button>
              </div>
            </form>

            {/* List of Saved Direct Tunnels */}
            {directTunnels.length > 0 && (
              <div className="space-y-2">
                <div className="text-xs font-semibold text-zinc-300">Saved Tunnel Tokens ({directTunnels.length})</div>
                <div className="space-y-2">
                  {directTunnels.map((dt) => {
                    const isRunning = Boolean(activeProcesses[dt.id]?.is_running);
                    return (
                      <div
                        key={dt.id}
                        className="p-3.5 rounded-xl bg-zinc-950 border border-zinc-800 flex items-center justify-between gap-3"
                      >
                        <div className="flex items-center gap-3 min-w-0">
                          <div
                            className={`w-2.5 h-2.5 rounded-full shrink-0 ${
                              isRunning ? "bg-emerald-500 animate-ping" : "bg-zinc-600"
                            }`}
                          />
                          <div className="min-w-0">
                            <div className="text-xs font-semibold text-zinc-200 truncate">{dt.name}</div>
                            <div className="text-[11px] text-zinc-500 font-mono truncate max-w-sm">
                              Token: {dt.token.substring(0, 16)}••••••••
                            </div>
                          </div>
                        </div>

                        <div className="flex items-center gap-2 shrink-0">
                          {isRunning ? (
                            <button
                              onClick={() => handleStopDirect(dt.id, dt.name)}
                              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-amber-950/40 hover:bg-amber-900/50 text-amber-300 border border-amber-800/60 text-xs font-medium transition cursor-pointer"
                            >
                              <Square className="w-3 h-3 fill-current" />
                              <span>Stop</span>
                            </button>
                          ) : (
                            <button
                              onClick={() => handleStartDirect(dt.id, dt.name, dt.token)}
                              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-950/40 hover:bg-emerald-900/50 text-emerald-300 border border-emerald-800/60 text-xs font-medium transition cursor-pointer"
                            >
                              <Play className="w-3 h-3 fill-current" />
                              <span>Start</span>
                            </button>
                          )}

                          <button
                            onClick={() => setDirectTunnelToDelete({ id: dt.id, name: dt.name })}
                            className="p-1.5 rounded-lg bg-zinc-900 hover:bg-rose-950/40 text-zinc-500 hover:text-rose-400 border border-zinc-800 transition cursor-pointer"
                            title="Delete saved token"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}

        {/* TAB 3: Cloudflare API Token */}
        {authTab === "api" && (
          <div className="space-y-4">
            <div className="flex items-start justify-between">
              <div className="space-y-1">
                <div className="text-xs font-semibold text-zinc-200 flex items-center gap-1.5">
                  <Key className="w-3.5 h-3.5 text-orange-400" />
                  <span>Scoped Cloudflare REST API Token</span>
                </div>
                <p className="text-xs text-zinc-400">
                  Encrypted inside your native operating system keychain. Enables visual ingress rule synchronization and automated DNS hygiene.
                </p>
              </div>

              <a
                href="https://dash.cloudflare.com/profile/api-tokens"
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1.5 text-xs text-blue-400 hover:text-blue-300 transition cursor-pointer"
              >
                <span>Create Token</span>
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
                      Authenticated &amp; Stored in OS Keychain
                    </div>
                    <div className="text-[11px] text-zinc-500 font-mono">
                      Token: ••••••••••••••••••••••••••••
                    </div>
                  </div>
                </div>

                <button
                  onClick={() => setShowConfirmApiLogout(true)}
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
                    className="absolute right-3 top-2.5 text-zinc-500 hover:text-zinc-300 transition cursor-pointer"
                  >
                    {showToken ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                  </button>
                </div>

                <div className="p-3 rounded-xl bg-zinc-950/60 border border-zinc-850 text-[11px] text-zinc-400 space-y-1">
                  <div className="font-semibold text-zinc-300">Required Scopes:</div>
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

      {/* Confirm Modal: Log out Origin Certificate */}
      <ConfirmModal
        isOpen={showConfirmCertLogout}
        title="Log Out (Delete Origin Certificate)"
        description="Are you sure you want to delete your origin certificate (~/.cloudflared/cert.pem)? You will need to re-authenticate via browser to manage cert-based tunnels."
        confirmLabel="Delete Certificate"
        onClose={() => setShowConfirmCertLogout(false)}
        onConfirm={async () => {
          await handleLogoutCert();
          setShowConfirmCertLogout(false);
        }}
      />

      {/* Confirm Modal: Remove API Token from Keychain */}
      <ConfirmModal
        isOpen={showConfirmApiLogout}
        title="Remove API Token"
        description="Are you sure you want to remove your Cloudflare REST API token from your OS Keychain? You will no longer be able to manage remote tunnels or automated DNS until you add a token back."
        confirmLabel="Remove Token"
        onClose={() => setShowConfirmApiLogout(false)}
        onConfirm={async () => {
          await handleLogoutApi();
          setShowConfirmApiLogout(false);
        }}
      />

      {/* Confirm Modal: Delete Direct Tunnel Token */}
      <ConfirmModal
        isOpen={Boolean(directTunnelToDelete)}
        title="Remove Saved Tunnel Token"
        description="This will stop the tunnel if it's currently running and delete the saved connector token from this machine."
        details={
          directTunnelToDelete && (
            <div className="space-y-1">
              <div className="font-semibold text-zinc-100">{directTunnelToDelete.name}</div>
              <div className="text-[11px] text-zinc-500 font-mono">ID: {directTunnelToDelete.id}</div>
            </div>
          )
        }
        confirmLabel="Remove Saved Tunnel"
        onClose={() => setDirectTunnelToDelete(null)}
        onConfirm={() => {
          if (directTunnelToDelete) {
            handleDeleteDirect(directTunnelToDelete.id, directTunnelToDelete.name);
            setDirectTunnelToDelete(null);
          }
        }}
      />
    </div>
  );
};
