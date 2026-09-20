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
} from "lucide-react";
import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import type { CloudflareTunnel } from "@/lib/tauri";

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
  const { token, activeAccountId } = useAuthStore();
  const {
    tunnels,
    activeProcesses,
    isLoading,
    error,
    fetchTunnels,
    createTunnel,
    startTunnel,
    stopTunnel,
    deleteTunnel,
  } = useTunnelStore();

  const [newTunnelName, setNewTunnelName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");

  useEffect(() => {
    if (activeAccountId) {
      fetchTunnels(activeAccountId);
    }
  }, [activeAccountId, fetchTunnels]);

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeAccountId || !newTunnelName.trim()) return;
    setIsCreating(true);
    const created = await createTunnel(activeAccountId, newTunnelName.trim());
    setIsCreating(false);
    if (created) {
      setNewTunnelName("");
      setShowCreateModal(false);
    }
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(text);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const filteredTunnels = tunnels.filter((t) =>
    t.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    t.id.toLowerCase().includes(searchQuery.toLowerCase())
  );

  if (!token) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-8 text-center">
        <div className="w-12 h-12 rounded-2xl bg-zinc-900 border border-zinc-800 flex items-center justify-center text-zinc-400 mb-4">
          <Network className="w-6 h-6" />
        </div>
        <h2 className="text-base font-semibold text-zinc-100">Connect Your Cloudflare Account</h2>
        <p className="text-xs text-zinc-400 max-w-sm mt-1 mb-5">
          To manage remotely-managed Zero Trust tunnels, configure your scoped Cloudflare API token in Settings.
        </p>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Top Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <Network className="w-5 h-5 text-blue-400" />
            Zero Trust Tunnels
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Manage, run, and supervise persistent Cloudflare tunnels configured on the edge.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={() => activeAccountId && fetchTunnels(activeAccountId)}
            disabled={isLoading}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Refresh tunnels"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={() => setShowCreateModal(true)}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>Create Tunnel</span>
          </button>
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

      {/* Tunnels Grid */}
      <div className="grid grid-cols-1 gap-3.5">
        {filteredTunnels.map((tunnel) => {
          const isRunningLocally = !!activeProcesses[tunnel.id];
          const hasConnections = tunnel.connections && tunnel.connections.length > 0;
          const statusText = isRunningLocally
            ? "Running (Local Host)"
            : hasConnections
            ? "Connected (Remote)"
            : "Inactive";

          return (
            <div
              key={tunnel.id}
              className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 hover:border-zinc-700/80 transition space-y-3.5"
            >
              <div className="flex items-start justify-between">
                <div className="space-y-1">
                  <div className="flex items-center gap-2.5">
                    <span className="font-semibold text-sm text-zinc-100">{tunnel.name}</span>
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
                      onClick={() => stopTunnel(tunnel.id)}
                      className="flex items-center gap-1.5 text-xs bg-rose-950/40 hover:bg-rose-900/50 text-rose-300 border border-rose-800/60 px-3 py-1.5 rounded-lg transition cursor-pointer font-medium"
                    >
                      <Square className="w-3.5 h-3.5 fill-current" />
                      <span>Stop</span>
                    </button>
                  ) : (
                    <button
                      onClick={() => activeAccountId && startTunnel(activeAccountId, tunnel.id)}
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
                  onClick={() => {
                    if (confirm(`Are you sure you want to delete tunnel "${tunnel.name}"?`)) {
                      if (activeAccountId) deleteTunnel(activeAccountId, tunnel.id);
                    }
                  }}
                  className="text-zinc-500 hover:text-rose-400 p-1 rounded transition cursor-pointer"
                  title="Delete Tunnel"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          );
        })}

        {filteredTunnels.length === 0 && (
          <div className="p-8 text-center rounded-xl bg-zinc-950 border border-zinc-850 text-zinc-500 text-xs">
            No tunnels found. Click &quot;Create Tunnel&quot; above to create a new Zero Trust tunnel.
          </div>
        )}
      </div>

      {/* Create Tunnel Modal */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="w-full max-w-md p-6 rounded-2xl bg-zinc-900 border border-zinc-800 shadow-2xl space-y-4">
            <h3 className="text-base font-bold text-zinc-100 flex items-center gap-2">
              <Plus className="w-4 h-4 text-blue-400" />
              Create Remotely-Managed Tunnel
            </h3>
            <p className="text-xs text-zinc-400">
              Cloudflare will provision this tunnel on the edge. You can run it locally with one click.
            </p>

            <form onSubmit={handleCreate} className="space-y-4">
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-zinc-300">Tunnel Name</label>
                <input
                  type="text"
                  required
                  placeholder="e.g. dev-cluster, staging-api, home-nas"
                  value={newTunnelName}
                  onChange={(e) => setNewTunnelName(e.target.value)}
                  className="w-full bg-zinc-950 border border-zinc-750 rounded-lg px-3.5 py-2 text-xs text-zinc-100 focus:border-blue-500 focus:outline-none"
                />
              </div>

              <div className="flex items-center justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setShowCreateModal(false)}
                  className="px-3.5 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={isCreating}
                  className="px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-60"
                >
                  {isCreating ? "Creating on Edge..." : "Create Tunnel"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
