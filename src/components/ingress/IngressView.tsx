import React, { useState, useEffect } from "react";
import {
  GitBranch,
  Plus,
  Trash2,
  Save,
  RefreshCw,
  Check,
  Shield,
  ArrowRight,
  AlertTriangle,
} from "lucide-react";

import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { tauriApi, type IngressRule, type CloudflareTunnel } from "@/lib/tauri";

interface IngressViewProps {
  initialTunnel?: CloudflareTunnel | null;
}

export const IngressView: React.FC<IngressViewProps> = ({ initialTunnel }) => {
  const { activeAccountId, token } = useAuthStore();
  const { tunnels } = useTunnelStore();

  const [selectedTunnelId, setSelectedTunnelId] = useState<string>(
    initialTunnel?.id || tunnels[0]?.id || ""
  );
  const [rules, setRules] = useState<IngressRule[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [savedSuccess, setSavedSuccess] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);

  // New Rule Form state
  const [newHostname, setNewHostname] = useState("");
  const [newPath, setNewPath] = useState("");
  const [newProtocol, setNewProtocol] = useState("http");
  const [newTarget, setNewTarget] = useState("localhost:3000");
  const [noTlsVerify, setNoTlsVerify] = useState(false);
  const [hostHeader, setHostHeader] = useState("");

  const loadRules = async (tunnelId: string) => {
    if (!activeAccountId || !tunnelId) return;
    setIsLoading(true);
    try {
      const config = await tauriApi.getTunnelConfiguration(activeAccountId, tunnelId, token || undefined);
      setRules(config.ingress || []);
    } catch (err) {
      console.error("Failed to load ingress configuration:", err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (selectedTunnelId) {
      loadRules(selectedTunnelId);
    }
  }, [selectedTunnelId, activeAccountId]);

  const handleSave = async () => {
    if (!activeAccountId || !selectedTunnelId) return;
    setIsSaving(true);
    try {
      await tauriApi.updateTunnelConfiguration(
        activeAccountId,
        selectedTunnelId,
        rules,
        token || undefined
      );
      setSavedSuccess(true);
      setTimeout(() => setSavedSuccess(false), 2500);
    } catch (err) {
      console.error("Failed to save configuration:", err);
    } finally {
      setIsSaving(false);
    }
  };

  const handleAddRule = (e: React.FormEvent) => {
    e.preventDefault();
    const serviceString = `${newProtocol}://${newTarget}`;
    const newRule: IngressRule = {
      hostname: newHostname.trim() ? newHostname.trim() : undefined,
      path: newPath.trim() ? newPath.trim() : undefined,
      service: serviceString,
      origin_request:
        noTlsVerify || hostHeader.trim()
          ? {
              no_tls_verify: noTlsVerify ? true : undefined,
              http_host_header: hostHeader.trim() ? hostHeader.trim() : undefined,
            }
          : undefined,
    };

    // Insert before the 404 catch-all rule if one exists
    const catchAllIndex = rules.findIndex((r) => r.service === "http_status:404");
    if (catchAllIndex !== -1) {
      const copy = [...rules];
      copy.splice(catchAllIndex, 0, newRule);
      setRules(copy);
    } else {
      setRules([...rules, newRule]);
    }

    // Reset
    setNewHostname("");
    setNewPath("");
    setNewTarget("localhost:3000");
    setNoTlsVerify(false);
    setHostHeader("");
    setShowAddModal(false);
  };

  const handleDeleteRule = (index: number) => {
    const updated = [...rules];
    updated.splice(index, 1);
    setRules(updated);
  };

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <GitBranch className="w-5 h-5 text-blue-400" />
            Visual Ingress Routing
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Route incoming hostnames and paths directly to local ports without manual YAML editing.
          </p>
        </div>

        <div className="flex items-center gap-2">
          {/* Select Tunnel */}
          <select
            value={selectedTunnelId}
            onChange={(e) => setSelectedTunnelId(e.target.value)}
            className="bg-zinc-900 border border-zinc-750 text-xs text-zinc-200 px-3 py-2 rounded-lg focus:outline-none cursor-pointer"
          >
            {tunnels.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>

          <button
            onClick={() => loadRules(selectedTunnelId)}
            disabled={isLoading}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Reload configuration from Cloudflare"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={() => setShowAddModal(true)}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-zinc-200 text-xs font-semibold border border-zinc-700 transition cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>Add Route</span>
          </button>

          <button
            onClick={handleSave}
            disabled={isSaving}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer disabled:opacity-60"
          >
            {savedSuccess ? (
              <>
                <Check className="w-4 h-4 text-emerald-300" />
                <span>Deployed!</span>
              </>
            ) : (
              <>
                <Save className="w-4 h-4" />
                <span>{isSaving ? "Deploying..." : "Save to Edge"}</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Routing Rules Table */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900/40 overflow-hidden">
        <table className="w-full text-left text-xs">
          <thead className="bg-zinc-900/80 border-b border-zinc-800 text-zinc-400 font-semibold">
            <tr>
              <th className="py-3 px-4">Priority</th>
              <th className="py-3 px-4">Hostname</th>
              <th className="py-3 px-4">Path Regex</th>
              <th className="py-3 px-4">Local Target / Service</th>
              <th className="py-3 px-4">Flags</th>
              <th className="py-3 px-4 text-right">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-850">
            {rules.map((rule, idx) => {
              const isCatchAll = rule.service === "http_status:404";
              return (
                <tr
                  key={idx}
                  className={`hover:bg-zinc-850/40 transition ${
                    isCatchAll ? "bg-zinc-950/40 opacity-75 font-mono" : ""
                  }`}
                >
                  <td className="py-3 px-4 font-mono text-zinc-500">#{idx + 1}</td>
                  <td className="py-3 px-4">
                    {rule.hostname ? (
                      <span className="font-semibold text-zinc-200">{rule.hostname}</span>
                    ) : (
                      <span className="text-zinc-500 italic">* (Any Hostname)</span>
                    )}
                  </td>
                  <td className="py-3 px-4 font-mono text-zinc-400">
                    {rule.path || <span className="text-zinc-600 italic">/* (All paths)</span>}
                  </td>
                  <td className="py-3 px-4">
                    <div className="flex items-center gap-1.5 font-mono text-blue-300">
                      <ArrowRight className="w-3.5 h-3.5 text-zinc-500" />
                      <span>{rule.service}</span>
                    </div>
                  </td>
                  <td className="py-3 px-4">
                    {rule.origin_request?.no_tls_verify && (
                      <span className="inline-flex items-center gap-1 text-[10px] bg-amber-950/50 text-amber-300 border border-amber-800/40 px-1.5 py-0.5 rounded">
                        <AlertTriangle className="w-3 h-3" />
                        NoTLSVerify
                      </span>
                    )}
                  </td>
                  <td className="py-3 px-4 text-right">
                    {!isCatchAll && (
                      <button
                        onClick={() => handleDeleteRule(idx)}
                        className="p-1 rounded text-zinc-500 hover:text-rose-400 transition cursor-pointer"
                        title="Delete Rule"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}

            {rules.length === 0 && (
              <tr>
                <td colSpan={6} className="py-8 text-center text-zinc-500 italic">
                  No ingress rules defined for this tunnel yet. Click &quot;Add Route&quot; to begin.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* Catch-all explanation note */}
      <div className="p-4 rounded-xl bg-zinc-950 border border-zinc-800 flex items-center justify-between text-xs text-zinc-400">
        <div className="flex items-center gap-2">
          <Shield className="w-4 h-4 text-emerald-400" />
          <span>
            <strong>Zero Trust Security:</strong> Any incoming request not matched by your rules will be safely dropped by the catch-all <code className="text-zinc-300">http_status:404</code> rule.
          </span>
        </div>
      </div>

      {/* Add Route Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="w-full max-w-lg p-6 rounded-2xl bg-zinc-900 border border-zinc-800 shadow-2xl space-y-4">
            <h3 className="text-base font-bold text-zinc-100 flex items-center gap-2">
              <Plus className="w-4 h-4 text-blue-400" />
              Add Ingress Route
            </h3>

            <form onSubmit={handleAddRule} className="space-y-4">
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1">
                  <label className="text-xs font-semibold text-zinc-300">Public Hostname</label>
                  <input
                    type="text"
                    placeholder="e.g. api.yourdomain.com"
                    value={newHostname}
                    onChange={(e) => setNewHostname(e.target.value)}
                    className="w-full bg-zinc-950 border border-zinc-750 rounded-lg px-3 py-2 text-xs text-zinc-100 focus:border-blue-500 focus:outline-none font-mono"
                  />
                </div>

                <div className="space-y-1">
                  <label className="text-xs font-semibold text-zinc-300">Path (Optional Regex)</label>
                  <input
                    type="text"
                    placeholder="e.g. /api/v1/*"
                    value={newPath}
                    onChange={(e) => setNewPath(e.target.value)}
                    className="w-full bg-zinc-950 border border-zinc-750 rounded-lg px-3 py-2 text-xs text-zinc-100 focus:border-blue-500 focus:outline-none font-mono"
                  />
                </div>
              </div>

              {/* Service Protocol and Target */}
              <div className="space-y-1">
                <label className="text-xs font-semibold text-zinc-300">Target Local Service</label>
                <div className="flex gap-2">
                  <select
                    value={newProtocol}
                    onChange={(e) => setNewProtocol(e.target.value)}
                    className="w-28 bg-zinc-950 border border-zinc-750 rounded-lg px-2.5 py-2 text-xs text-zinc-100 font-mono focus:outline-none"
                  >
                    <option value="http">http://</option>
                    <option value="https">https://</option>
                    <option value="tcp">tcp://</option>
                    <option value="ssh">ssh://</option>
                    <option value="rdp">rdp://</option>
                    <option value="unix">unix:</option>
                  </select>

                  <input
                    type="text"
                    required
                    placeholder="localhost:3000"
                    value={newTarget}
                    onChange={(e) => setNewTarget(e.target.value)}
                    className="flex-1 bg-zinc-950 border border-zinc-750 rounded-lg px-3 py-2 text-xs text-zinc-100 focus:border-blue-500 focus:outline-none font-mono"
                  />
                </div>
              </div>

              {/* Advanced Flags */}
              <div className="p-3 rounded-xl bg-zinc-950/70 border border-zinc-800/80 space-y-2.5">
                <div className="flex items-center justify-between text-xs">
                  <span className="text-zinc-300 font-medium">Bypass Upstream TLS Verify</span>
                  <input
                    type="checkbox"
                    checked={noTlsVerify}
                    onChange={(e) => setNoTlsVerify(e.target.checked)}
                    className="rounded text-blue-600 focus:ring-0 cursor-pointer"
                  />
                </div>

                <div className="space-y-1">
                  <label className="text-[11px] text-zinc-400">Custom HTTP Host Header (Override)</label>
                  <input
                    type="text"
                    placeholder="e.g. internal.local"
                    value={hostHeader}
                    onChange={(e) => setHostHeader(e.target.value)}
                    className="w-full bg-zinc-900 border border-zinc-800 rounded-md px-2.5 py-1.5 text-xs text-zinc-200 font-mono focus:outline-none"
                  />
                </div>
              </div>

              <div className="flex items-center justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setShowAddModal(false)}
                  className="px-3.5 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer"
                >
                  Add Rule
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
