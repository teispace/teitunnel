import React, { useState, useEffect } from "react";
import {
  Network,
  Plus,
  Play,
  Square,
  RefreshCw,
  GitBranch,
  Trash2,
  Copy,
  Check,
  Globe,
  Activity,
  AlertCircle,
  Zap,
  Shield,
  Eye,
  EyeOff,
} from "lucide-react";
import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { toast } from "sonner";
import type { CloudflareTunnel, DirectTunnel } from "@/lib/tauri";
import { ConfirmModal } from "@/components/ui/ConfirmModal";
import { Modal } from "@/components/ui/Modal";

interface TunnelsViewProps {
  onNavigateIngress: (tunnel: CloudflareTunnel) => void;
  onNavigateDns: (tunnel: CloudflareTunnel) => void;
  onNavigateTelemetry: (tunnel: CloudflareTunnel) => void;
}

export const TunnelsView: React.FC<TunnelsViewProps> = ({
  onNavigateIngress,
  onNavigateDns,
  onNavigateTelemetry,
}) => {
  const {
    token,
    activeAccountId,
    certStatus,
    startBrowserLogin,
    isLoggingInBrowser,
  } = useAuthStore();

  const {
    tunnels,
    directTunnels,
    activeProcesses,
    isLoading,
    error,
    fetchTunnels,
    fetchCertTunnels,
    createTunnel,
    createCertTunnel,
    startTunnel,
    startNamedTunnel,
    startDirectTunnel,
    stopTunnel,
    deleteTunnel,
    deleteCertTunnel,
    saveDirectTunnel,
    deleteDirectTunnel,
    refreshProcesses,
  } = useTunnelStore();

  const [newTunnelName, setNewTunnelName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [showCreateModal, setShowCreateModal] = useState(false);

  // Direct Token Modal state
  const [showTokenModal, setShowTokenModal] = useState(false);
  const [tokenModalName, setTokenModalName] = useState("");
  const [tokenModalValue, setTokenModalValue] = useState("");
  const [showModalToken, setShowModalToken] = useState(false);

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");

  // Native Confirm Dialog states
  const [tunnelToDelete, setTunnelToDelete] = useState<CloudflareTunnel | null>(null);
  const [directTunnelToDelete, setDirectTunnelToDelete] = useState<DirectTunnel | null>(null);
  const [isDeletingTunnel, setIsDeletingTunnel] = useState(false);

  const hasAnyAuth = Boolean(certStatus?.has_cert || token || directTunnels.length > 0);

  const refreshAll = async () => {
    if (activeAccountId && token) {
      await fetchTunnels(activeAccountId, token);
    } else if (certStatus?.has_cert) {
      await fetchCertTunnels();
    }
    await refreshProcesses();
  };

  useEffect(() => {
    refreshAll();
  }, [certStatus?.has_cert, activeAccountId, token]);

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newTunnelName.trim();
    if (!name) return;
    setIsCreating(true);

    let res: CloudflareTunnel | null = null;
    if (certStatus?.has_cert) {
      res = await createCertTunnel(name);
    } else if (activeAccountId) {
      res = await createTunnel(activeAccountId, name);
    }

    setIsCreating(false);
    if (res) {
      toast.success(`Created tunnel "${name}"`);
      setNewTunnelName("");
      setShowCreateModal(false);
    } else {
      const err = useTunnelStore.getState().error || "Failed to create tunnel";
      toast.error(`Create failed: ${err}`);
    }
  };

  const handleSaveAndRunDirectToken = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!tokenModalValue.trim()) return;

    const saved = saveDirectTunnel(
      tokenModalName.trim() || "Zero Trust Tunnel",
      tokenModalValue.trim()
    );

    // Immediately start the newly added direct tunnel
    await startDirectTunnel(saved.id, saved.token);
    toast.success(`Saved and launched tunnel "${saved.name}"`);

    setTokenModalName("");
    setTokenModalValue("");
    setShowTokenModal(false);
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(text);
    setTimeout(() => setCopiedId(null), 2000);
  };

  // Combine standard tunnels and direct token tunnels for unified listing
  const filteredTunnels = tunnels.filter((t) =>
    t.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    t.id.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const filteredDirectTunnels = directTunnels.filter((dt) =>
    dt.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    dt.id.toLowerCase().includes(searchQuery.toLowerCase())
  );

  // If no auth and no direct tunnels saved, show welcome cards
  if (!hasAnyAuth) {
    return (
      <div className="flex-1 overflow-y-auto p-8 flex flex-col items-center justify-center max-w-3xl mx-auto space-y-6">
        <div className="text-center space-y-2">
          <div className="w-14 h-14 rounded-2xl bg-blue-600/10 border border-blue-500/20 flex items-center justify-center text-blue-400 mx-auto">
            <Network className="w-7 h-7" />
          </div>
          <h2 className="text-xl font-bold text-zinc-100">Zero Trust Tunnels</h2>
          <p className="text-xs text-zinc-400 max-w-md">
            Connect persistent, production-ready Cloudflare tunnels to your local machine. Choose the simplest method that fits your workflow:
          </p>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 w-full">
          {/* Card 1: 1-Click Browser Login */}
          <div className="p-5 rounded-2xl bg-zinc-900/60 border border-zinc-800 hover:border-blue-500/50 transition flex flex-col justify-between space-y-4">
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Globe className="w-4 h-4 text-blue-400" />
                <span className="font-semibold text-sm text-zinc-100">1-Click Browser Login</span>
                <span className="text-[10px] uppercase font-bold px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-300">
                  Recommended
                </span>
              </div>
              <p className="text-xs text-zinc-400">
                Authorize your domain in the browser. Automatically downloads <code className="text-zinc-300 font-mono">cert.pem</code> so you can create and run named tunnels with zero API keys.
              </p>
            </div>

            <button
              onClick={startBrowserLogin}
              disabled={isLoggingInBrowser}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-50"
            >
              <Globe className="w-3.5 h-3.5" />
              <span>{isLoggingInBrowser ? "Waiting for Browser..." : "Log in with Cloudflare"}</span>
            </button>
          </div>

          {/* Card 2: Direct Tunnel Token */}
          <div className="p-5 rounded-2xl bg-zinc-900/60 border border-zinc-800 hover:border-amber-500/50 transition flex flex-col justify-between space-y-4">
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Zap className="w-4 h-4 text-amber-400" />
                <span className="font-semibold text-sm text-zinc-100">Direct Tunnel Token</span>
                <span className="text-[10px] uppercase font-bold px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-300">
                  Instant
                </span>
              </div>
              <p className="text-xs text-zinc-400">
                Already have a tunnel in Cloudflare Zero Trust dashboard? Simply paste the connector token (<code className="text-zinc-300 font-mono">eyJh...</code>) and run it right now.
              </p>
            </div>

            <button
              onClick={() => setShowTokenModal(true)}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl bg-zinc-800 hover:bg-zinc-700 text-zinc-100 text-xs font-semibold border border-zinc-700 transition cursor-pointer"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>Paste Tunnel Token</span>
            </button>
          </div>
        </div>

        {/* Create Direct Token Modal */}
        <Modal
          isOpen={showTokenModal}
          onClose={() => setShowTokenModal(false)}
          title="Run Tunnel via Connector Token"
          description="Paste the connector token (eyJh...) from your Cloudflare Zero Trust dashboard."
          icon={<Zap className="w-4 h-4 text-amber-400" />}
        >
          <form onSubmit={handleSaveAndRunDirectToken} className="space-y-4">
            <div className="space-y-1.5">
              <label className="text-xs font-semibold text-zinc-300">Tunnel Name / Label</label>
              <input
                type="text"
                placeholder="e.g. My App / Home Server"
                value={tokenModalName}
                onChange={(e) => setTokenModalName(e.target.value)}
                className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none"
              />
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-semibold text-zinc-300">Connector Token</label>
              <div className="relative">
                <input
                  type={showModalToken ? "text" : "password"}
                  required
                  placeholder="eyJh..."
                  value={tokenModalValue}
                  onChange={(e) => setTokenModalValue(e.target.value)}
                  className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 pr-9 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                />
                <button
                  type="button"
                  onClick={() => setShowModalToken(!showModalToken)}
                  className="absolute right-2.5 top-2.5 text-zinc-500 hover:text-zinc-300 transition cursor-pointer"
                >
                  {showModalToken ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                </button>
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setShowTokenModal(false)}
                className="px-4 py-2 rounded-xl bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={!tokenModalValue.trim()}
                className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-60"
              >
                Start &amp; Save Tunnel
              </button>
            </div>
          </form>
        </Modal>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Top Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
              <Network className="w-5 h-5 text-blue-400" />
              Zero Trust Tunnels
            </h1>
            {certStatus?.has_cert && (
              <span className="inline-flex items-center gap-1 text-[10px] font-semibold px-2 py-0.5 rounded-full bg-emerald-950/60 text-emerald-400 border border-emerald-800/60">
                <Shield className="w-3 h-3" />
                Origin Cert
              </span>
            )}
          </div>
          <p className="text-xs text-zinc-400 mt-0.5">
            Manage, run, and supervise persistent Cloudflare tunnels configured locally or on the edge.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={refreshAll}
            disabled={isLoading}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Refresh tunnels"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={() => setShowTokenModal(true)}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-zinc-100 border border-zinc-700 text-xs font-semibold transition cursor-pointer"
          >
            <Zap className="w-3.5 h-3.5 text-amber-400" />
            <span>+ Run with Token</span>
          </button>

          {(certStatus?.has_cert || token) && (
            <button
              onClick={() => setShowCreateModal(true)}
              className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer"
            >
              <Plus className="w-4 h-4" />
              <span>Create Tunnel</span>
            </button>
          )}
        </div>
      </div>

      {/* Search Filter */}
      <div className="flex items-center gap-3">
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Filter tunnels by name or UUID..."
          className="flex-1 bg-zinc-950 border border-zinc-800 rounded-lg px-3.5 py-2 text-xs text-zinc-200 placeholder-zinc-500 focus:border-blue-500 focus:outline-none"
        />
      </div>

      {error && (
        <div className="p-3.5 rounded-xl bg-rose-950/30 border border-rose-800/50 flex items-center gap-2.5 text-xs text-rose-300">
          <AlertCircle className="w-4 h-4 text-rose-400 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {/* Combined Tunnels Grid */}
      <div className="grid grid-cols-1 gap-3.5">
        {/* 1. Direct Token Tunnels */}
        {filteredDirectTunnels.map((dt) => {
          const isRunningLocally = Boolean(activeProcesses[dt.id]?.is_running);
          return (
            <div
              key={dt.id}
              className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 hover:border-zinc-700/80 transition space-y-3.5"
            >
              <div className="flex items-start justify-between">
                <div className="space-y-1">
                  <div className="flex items-center gap-2.5">
                    <span className="font-semibold text-sm text-zinc-100">{dt.name}</span>
                    <span className="text-[10px] uppercase font-bold px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-300 border border-amber-800/40">
                      Tunnel Token
                    </span>
                    <span
                      className={`inline-flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded-full border ${
                        isRunningLocally
                          ? "bg-emerald-950/50 text-emerald-400 border-emerald-800/60"
                          : "bg-zinc-800/60 text-zinc-400 border-zinc-700/50"
                      }`}
                    >
                      <span
                        className={`w-1.5 h-1.5 rounded-full ${
                          isRunningLocally ? "bg-emerald-400 animate-pulse" : "bg-zinc-500"
                        }`}
                      />
                      {isRunningLocally ? "Active & Forwarding" : "Idle"}
                    </span>
                  </div>

                  <div className="text-[11px] text-zinc-500 font-mono">
                    Token: {dt.token.substring(0, 20)}••••••••••••••••••••••••••••••••••••
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  {isRunningLocally ? (
                    <button
                      onClick={() => stopTunnel(dt.id)}
                      className="flex items-center gap-1.5 text-xs bg-rose-950/40 hover:bg-rose-900/50 text-rose-300 border border-rose-800/60 px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
                    >
                      <Square className="w-3.5 h-3.5 fill-current" />
                      <span>Stop</span>
                    </button>
                  ) : (
                    <button
                      onClick={() => startDirectTunnel(dt.id, dt.token)}
                      className="flex items-center gap-1.5 text-xs bg-emerald-950/40 hover:bg-emerald-900/50 text-emerald-300 border border-emerald-800/60 px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
                    >
                      <Play className="w-3.5 h-3.5 fill-current" />
                      <span>Start Tunnel</span>
                    </button>
                  )}
                </div>
              </div>

              {/* Action Toolbar for Direct Tunnel */}
              <div className="pt-2 border-t border-zinc-800/70 flex items-center justify-between">
                {isRunningLocally ? (
                  <button
                    onClick={() =>
                      onNavigateTelemetry({
                        id: dt.id,
                        name: dt.name,
                        connections: [],
                        remote_config: true,
                      })
                    }
                    className="flex items-center gap-1.5 text-xs bg-zinc-800/70 hover:bg-zinc-700/80 text-zinc-200 px-2.5 py-1 rounded-md border border-zinc-700/50 transition cursor-pointer"
                  >
                    <Activity className="w-3.5 h-3.5 text-purple-400" />
                    <span>Telemetry</span>
                  </button>
                ) : (
                  <span className="text-[11px] text-zinc-500">Run to view live telemetry</span>
                )}

                <button
                  onClick={() => setDirectTunnelToDelete(dt)}
                  className="text-zinc-500 hover:text-rose-400 p-1.5 rounded-lg hover:bg-rose-950/30 transition cursor-pointer"
                  title="Remove Saved Tunnel"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          );
        })}

        {/* 2. Standard Tunnels (Cert or API) */}
        {filteredTunnels.map((tunnel) => {
          const isRunningLocally = Boolean(
            activeProcesses[tunnel.id] || activeProcesses[tunnel.name]
          );
          const hasConnections = tunnel.connections && tunnel.connections.length > 0;
          const statusText = isRunningLocally
            ? "Running (Local Host)"
            : hasConnections
            ? "Connected (Remote Edge)"
            : "Inactive";

          const isCertManaged = Boolean(certStatus?.has_cert && !tunnel.remote_config);

          return (
            <div
              key={tunnel.id}
              className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 hover:border-zinc-700/80 transition space-y-3.5"
            >
              <div className="flex items-start justify-between">
                <div className="space-y-1">
                  <div className="flex items-center gap-2.5">
                    <span className="font-semibold text-sm text-zinc-100">{tunnel.name}</span>
                    <span className="text-[10px] uppercase font-bold px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-300 border border-blue-800/40">
                      {tunnel.remote_config ? "Remote Zero Trust" : "Origin Cert"}
                    </span>
                    <span
                      className={`inline-flex items-center gap-1 text-[10px] font-medium px-2 py-0.5 rounded-full border ${
                        isRunningLocally || hasConnections
                          ? "bg-emerald-950/50 text-emerald-400 border-emerald-800/60"
                          : "bg-zinc-800/60 text-zinc-400 border-zinc-700/50"
                      }`}
                    >
                      <span
                        className={`w-1.5 h-1.5 rounded-full ${
                          isRunningLocally || hasConnections ? "bg-emerald-400" : "bg-zinc-500"
                        }`}
                      />
                      {statusText}
                    </span>
                  </div>

                  {/* UUID with copy */}
                  <div className="flex items-center gap-1.5 text-xs text-zinc-500 font-mono">
                    <span>{tunnel.id}</span>
                    <button
                      onClick={() => handleCopy(tunnel.id)}
                      className="text-zinc-500 hover:text-zinc-300 transition"
                      title="Copy Tunnel UUID"
                    >
                      {copiedId === tunnel.id ? (
                        <Check className="w-3 h-3 text-emerald-400" />
                      ) : (
                        <Copy className="w-3 h-3" />
                      )}
                    </button>
                  </div>
                </div>

                {/* Local Start / Stop Button */}
                <div className="flex items-center gap-2">
                  {isRunningLocally ? (
                    <button
                      onClick={async () => {
                        const target = isCertManaged ? tunnel.name : tunnel.id;
                        const ok = await stopTunnel(target);
                        if (ok) {
                          toast.info(`Stopped tunnel "${tunnel.name}"`);
                        } else {
                          const err = useTunnelStore.getState().error || "Failed to stop tunnel";
                          toast.error(`Stop failed: ${err}`);
                        }
                      }}
                      className="flex items-center gap-1.5 text-xs bg-rose-950/40 hover:bg-rose-900/50 text-rose-300 border border-rose-800/60 px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
                    >
                      <Square className="w-3.5 h-3.5 fill-current" />
                      <span>Stop</span>
                    </button>
                  ) : (
                    <button
                      onClick={async () => {
                        const toastId = toast.loading(`Starting tunnel "${tunnel.name}"...`);
                        let ok = false;
                        if (isCertManaged) {
                          ok = await startNamedTunnel(tunnel.name);
                        } else if (activeAccountId) {
                          ok = await startTunnel(activeAccountId, tunnel.id, token || undefined);
                        } else {
                          ok = await startNamedTunnel(tunnel.name);
                        }

                        if (ok) {
                          toast.success(`Tunnel "${tunnel.name}" started`, { id: toastId });
                        } else {
                          const err = useTunnelStore.getState().error || "Failed to start tunnel";
                          toast.error(`Start failed: ${err}`, { id: toastId });
                        }
                      }}
                      className="flex items-center gap-1.5 text-xs bg-emerald-950/40 hover:bg-emerald-900/50 text-emerald-300 border border-emerald-800/60 px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
                    >
                      <Play className="w-3.5 h-3.5 fill-current" />
                      <span>Start on Machine</span>
                    </button>
                  )}
                </div>
              </div>

              {/* Action Toolbar */}
              <div className="pt-2 border-t border-zinc-800/70 flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  {token && (
                    <>
                      <button
                        onClick={() => onNavigateIngress(tunnel)}
                        className="flex items-center gap-1.5 text-xs bg-zinc-800/70 hover:bg-zinc-700/80 text-zinc-200 px-2.5 py-1 rounded-md border border-zinc-700/50 transition cursor-pointer"
                      >
                        <GitBranch className="w-3.5 h-3.5 text-blue-400" />
                        <span>Ingress Rules</span>
                      </button>

                      <button
                        onClick={() => onNavigateDns(tunnel)}
                        className="flex items-center gap-1.5 text-xs bg-zinc-800/70 hover:bg-zinc-700/80 text-zinc-200 px-2.5 py-1 rounded-md border border-zinc-700/50 transition cursor-pointer"
                      >
                        <Globe className="w-3.5 h-3.5 text-emerald-400" />
                        <span>Link DNS</span>
                      </button>
                    </>
                  )}

                  {isRunningLocally && (
                    <button
                      onClick={() => onNavigateTelemetry(tunnel)}
                      className="flex items-center gap-1.5 text-xs bg-zinc-800/70 hover:bg-zinc-700/80 text-zinc-200 px-2.5 py-1 rounded-md border border-zinc-700/50 transition cursor-pointer"
                    >
                      <Activity className="w-3.5 h-3.5 text-purple-400" />
                      <span>Telemetry</span>
                    </button>
                  )}
                </div>

                <button
                  onClick={() => setTunnelToDelete(tunnel)}
                  className="text-zinc-500 hover:text-rose-400 p-1.5 rounded-lg hover:bg-rose-950/30 transition cursor-pointer"
                  title="Delete Tunnel"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          );
        })}

        {filteredTunnels.length === 0 && filteredDirectTunnels.length === 0 && (
          <div className="p-8 text-center rounded-xl bg-zinc-950 border border-zinc-850 text-zinc-500 text-xs">
            No tunnels found matching your filter. Use &quot;+ Run with Token&quot; or &quot;Create Tunnel&quot; above to add one.
          </div>
        )}
      </div>

      {/* Modal: Direct Tunnel Token */}
      <Modal
        isOpen={showTokenModal}
        onClose={() => setShowTokenModal(false)}
        title="Run Tunnel via Connector Token"
        description="Paste the connector token (eyJh...) from your Cloudflare Zero Trust dashboard."
        icon={<Zap className="w-4 h-4 text-amber-400" />}
      >
        <form onSubmit={handleSaveAndRunDirectToken} className="space-y-4">
          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-zinc-300">Tunnel Name / Label</label>
            <input
              type="text"
              placeholder="e.g. My App / Home Server"
              value={tokenModalName}
              onChange={(e) => setTokenModalName(e.target.value)}
              className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none"
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-zinc-300">Connector Token</label>
            <div className="relative">
              <input
                type={showModalToken ? "text" : "password"}
                required
                placeholder="eyJh..."
                value={tokenModalValue}
                onChange={(e) => setTokenModalValue(e.target.value)}
                className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 pr-9 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
              />
              <button
                type="button"
                onClick={() => setShowModalToken(!showModalToken)}
                className="absolute right-2.5 top-2.5 text-zinc-500 hover:text-zinc-300 transition cursor-pointer"
              >
                {showModalToken ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
              </button>
            </div>
          </div>

          <div className="flex items-center justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={() => setShowTokenModal(false)}
              className="px-4 py-2 rounded-xl bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!tokenModalValue.trim()}
              className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-60"
            >
              Start &amp; Save Tunnel
            </button>
          </div>
        </form>
      </Modal>

      {/* Modal: Create Named Tunnel */}
      <Modal
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        title="Create Named Tunnel"
        description="Registers a new named tunnel with Cloudflare. You can run and route traffic through it immediately."
        icon={<Plus className="w-4 h-4 text-blue-400" />}
      >
        <form onSubmit={handleCreate} className="space-y-4">
          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-zinc-300">Tunnel Name</label>
            <input
              type="text"
              required
              placeholder="e.g. dev-cluster, staging-api, home-nas"
              value={newTunnelName}
              onChange={(e) => setNewTunnelName(e.target.value)}
              className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none"
            />
          </div>

          <div className="flex items-center justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={() => setShowCreateModal(false)}
              className="px-4 py-2 rounded-xl bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isCreating || !newTunnelName.trim()}
              className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-60"
            >
              {isCreating ? "Creating..." : "Create Tunnel"}
            </button>
          </div>
        </form>
      </Modal>

      {/* Native Confirm Modal: Delete Standard Tunnel */}
      <ConfirmModal
        isOpen={Boolean(tunnelToDelete)}
        title="Delete Tunnel"
        description="Are you sure you want to permanently delete this tunnel? This will terminate running local instances, revoke origin credentials, and remove it from Cloudflare."
        details={
          tunnelToDelete && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-zinc-100">{tunnelToDelete.name}</span>
                <span className="text-[10px] uppercase font-bold px-2 py-0.5 rounded bg-blue-500/20 text-blue-300 border border-blue-800/40">
                  {tunnelToDelete.remote_config ? "Remote Zero Trust" : "Origin Cert"}
                </span>
              </div>
              <div className="text-[11px] text-zinc-500 font-mono select-all">
                UUID: {tunnelToDelete.id}
              </div>
            </div>
          )
        }
        confirmLabel="Delete Tunnel"
        isLoading={isDeletingTunnel}
        onClose={() => !isDeletingTunnel && setTunnelToDelete(null)}
        onConfirm={async () => {
          if (!tunnelToDelete) return;
          setIsDeletingTunnel(true);
          const toastId = toast.loading(`Deleting tunnel "${tunnelToDelete.name}"...`);
          try {
            let ok = false;
            // 1. If cert is available or local tunnel, delete via cert first
            if (certStatus?.has_cert || !tunnelToDelete.remote_config) {
              ok = await deleteCertTunnel(tunnelToDelete.id);
            }
            // 2. If not succeeded or account is available, delete via Cloudflare API
            if (!ok && activeAccountId) {
              ok = await deleteTunnel(activeAccountId, tunnelToDelete.id, token || undefined);
            } else if (!ok) {
              ok = await deleteTunnel("", tunnelToDelete.id, token || undefined);
            }

            if (ok) {
              toast.success(`Tunnel "${tunnelToDelete.name}" deleted successfully`, { id: toastId });
              setTunnelToDelete(null);
            } else {
              const errMsg = useTunnelStore.getState().error || "Failed to delete tunnel";
              toast.error(`Delete failed: ${errMsg}`, { id: toastId });
            }
          } finally {
            setIsDeletingTunnel(false);
          }
        }}
      />

      {/* Native Confirm Modal: Remove Direct Token */}
      <ConfirmModal
        isOpen={Boolean(directTunnelToDelete)}
        title="Remove Saved Tunnel"
        description="This will stop any active process running with this connector token and remove the saved credentials from your machine."
        details={
          directTunnelToDelete && (
            <div className="space-y-1">
              <div className="font-semibold text-zinc-100">{directTunnelToDelete.name}</div>
              <div className="text-[11px] text-zinc-500 font-mono">
                ID: {directTunnelToDelete.id}
              </div>
            </div>
          )
        }
        confirmLabel="Remove Saved Tunnel"
        onClose={() => setDirectTunnelToDelete(null)}
        onConfirm={async () => {
          if (!directTunnelToDelete) return;
          await deleteDirectTunnel(directTunnelToDelete.id);
          toast.success(`Removed saved tunnel "${directTunnelToDelete.name}"`);
          setDirectTunnelToDelete(null);
        }}
      />
    </div>
  );
};
