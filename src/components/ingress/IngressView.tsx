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
  ChevronUp,
  ChevronDown,
  Globe,
  Radio,
  Terminal,
  HardDrive,
  Code,
  Server,
  Zap,
} from "lucide-react";
import { toast } from "sonner";
import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { SelectDropdown, type SelectOption } from "@/components/ui/SelectDropdown";
import { Modal } from "@/components/ui/Modal";
import { ConfirmModal } from "@/components/ui/ConfirmModal";
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
  const [ruleToDeleteIndex, setRuleToDeleteIndex] = useState<number | null>(null);

  // New Rule Form state
  const [newHostname, setNewHostname] = useState("");
  const [newPath, setNewPath] = useState("");
  const [newProtocol, setNewProtocol] = useState("http");
  const [newTarget, setNewTarget] = useState("localhost:3000");
  const [noTlsVerify, setNoTlsVerify] = useState(false);
  const [hostHeader, setHostHeader] = useState("");

  const selectedTunnel = tunnels.find((t) => t.id === selectedTunnelId);

  // Keep selected tunnel in sync if tunnels change
  useEffect(() => {
    if (!selectedTunnelId && tunnels.length > 0) {
      setSelectedTunnelId(tunnels[0].id);
    }
  }, [tunnels, selectedTunnelId]);

  const loadRules = async (tunnelId: string) => {
    if (!activeAccountId || !tunnelId) return;
    setIsLoading(true);
    try {
      const config = await tauriApi.getTunnelConfiguration(
        activeAccountId,
        tunnelId,
        token || undefined
      );
      setRules(config.ingress || []);
    } catch (err) {
      console.error("Failed to load ingress configuration:", err);
      toast.error("Failed to load ingress configuration from Cloudflare.");
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (selectedTunnelId && activeAccountId) {
      loadRules(selectedTunnelId);
    }
  }, [selectedTunnelId, activeAccountId]);

  const handleSave = async () => {
    if (!activeAccountId || !selectedTunnelId) {
      toast.error("Please select a tunnel first.");
      return;
    }
    setIsSaving(true);
    try {
      await tauriApi.updateTunnelConfiguration(
        activeAccountId,
        selectedTunnelId,
        rules,
        token || undefined
      );
      setSavedSuccess(true);
      toast.success("Ingress rules deployed to Cloudflare Edge!");
      setTimeout(() => setSavedSuccess(false), 2500);
    } catch (err) {
      console.error("Failed to save configuration:", err);
      toast.error(err instanceof Error ? err.message : "Failed to deploy configuration");
    } finally {
      setIsSaving(false);
    }
  };

  const handleAddRule = (e: React.FormEvent) => {
    e.preventDefault();
    const serviceString = `${newProtocol}://${newTarget.trim()}`;
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

    toast.success(`Added route for ${newHostname || "*"}`);
    // Reset
    setNewHostname("");
    setNewPath("");
    setNewTarget("localhost:3000");
    setNoTlsVerify(false);
    setHostHeader("");
    setShowAddModal(false);
  };

  const handleDeleteRule = (index: number) => {
    const deletedRule = rules[index];
    const updated = [...rules];
    updated.splice(index, 1);
    setRules(updated);
    toast.success(
      `Removed route #${index + 1} (${deletedRule?.hostname || deletedRule?.service})`
    );
  };

  const handleMoveUp = (index: number) => {
    if (index <= 0) return;
    const newRules = [...rules];
    const temp = newRules[index];
    newRules[index] = newRules[index - 1];
    newRules[index - 1] = temp;
    setRules(newRules);
    toast.info(`Moved rule #${index + 1} up to #${index}`);
  };

  const handleMoveDown = (index: number) => {
    const catchAllIndex = rules.findIndex((r) => r.service === "http_status:404");
    const maxIndex = catchAllIndex !== -1 ? catchAllIndex - 1 : rules.length - 1;
    if (index >= maxIndex) return;
    const newRules = [...rules];
    const temp = newRules[index];
    newRules[index] = newRules[index + 1];
    newRules[index + 1] = temp;
    setRules(newRules);
    toast.info(`Moved rule #${index + 1} down to #${index + 2}`);
  };

  const applyPreset = (protocol: string, target: string, bypassTls = false) => {
    setNewProtocol(protocol);
    setNewTarget(target);
    setNoTlsVerify(bypassTls);
    toast.info(`Preset selected: ${protocol}://${target}`);
  };

  const tunnelOptions: SelectOption[] = tunnels.map((t) => ({
    value: t.id,
    label: t.name,
    sublabel: `ID: ${t.id.slice(0, 8)}...`,
    badge: t.status === "healthy" ? "Online" : t.status || "Tunnel",
    badgeColor:
      t.status === "healthy"
        ? "bg-emerald-950/60 text-emerald-300 border-emerald-800/40"
        : "bg-zinc-800 text-zinc-400 border-zinc-750",
    icon: <Server className="w-3.5 h-3.5 text-blue-400" />,
  }));

  const protocolOptions: SelectOption[] = [
    {
      value: "http",
      label: "http://",
      sublabel: "Standard HTTP Server",
      icon: <Globe className="w-3.5 h-3.5 text-blue-400" />,
    },
    {
      value: "https",
      label: "https://",
      sublabel: "TLS Upstream",
      icon: <Shield className="w-3.5 h-3.5 text-emerald-400" />,
    },
    {
      value: "tcp",
      label: "tcp://",
      sublabel: "Raw TCP Stream",
      icon: <Radio className="w-3.5 h-3.5 text-amber-400" />,
    },
    {
      value: "ssh",
      label: "ssh://",
      sublabel: "Secure Shell (Port 22)",
      icon: <Terminal className="w-3.5 h-3.5 text-purple-400" />,
    },
    {
      value: "rdp",
      label: "rdp://",
      sublabel: "Remote Desktop Protocol",
      icon: <HardDrive className="w-3.5 h-3.5 text-rose-400" />,
    },
    {
      value: "unix",
      label: "unix:",
      sublabel: "UNIX Socket Domain",
      icon: <Code className="w-3.5 h-3.5 text-zinc-400" />,
    },
  ];

  const catchAllIndex = rules.findIndex((r) => r.service === "http_status:404");
  const maxMovableIndex = catchAllIndex !== -1 ? catchAllIndex - 1 : rules.length - 1;

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <GitBranch className="w-5 h-5 text-blue-400" />
            <span>Visual Ingress Routing</span>
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Configure ingress routes, reorder rule priority, and deploy changes directly to Cloudflare edge.
          </p>
        </div>

        <div className="flex items-center gap-2">
          {/* Target Tunnel Selector */}
          {tunnels.length > 0 && (
            <SelectDropdown
              options={tunnelOptions}
              value={selectedTunnelId}
              onChange={(val) => setSelectedTunnelId(val)}
              searchable={tunnels.length > 3}
              placeholder="Select Tunnel..."
              triggerClassName="h-9 px-3 bg-zinc-900 border-zinc-800"
              menuClassName="min-w-[240px]"
            />
          )}

          <button
            onClick={() => {
              if (selectedTunnelId) {
                loadRules(selectedTunnelId);
                toast.info("Refreshed ingress rules");
              }
            }}
            disabled={isLoading || !selectedTunnelId}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Reload configuration from Cloudflare"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={() => {
              if (!selectedTunnelId) {
                toast.error("Please select a tunnel first.");
                return;
              }
              setShowAddModal(true);
            }}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-750 text-zinc-200 text-xs font-semibold border border-zinc-700 transition cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>Add Route</span>
          </button>

          <button
            onClick={handleSave}
            disabled={isSaving || !selectedTunnelId}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer disabled:opacity-50"
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

      {/* Selected Tunnel Details Banner */}
      {selectedTunnel && (
        <div className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800/80 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-blue-950/50 border border-blue-800/40 flex items-center justify-center text-blue-400">
              <Server className="w-4 h-4" />
            </div>
            <div>
              <div className="font-semibold text-zinc-200 flex items-center gap-2">
                <span>{selectedTunnel.name}</span>
                <span className="text-[10px] font-mono text-zinc-400 px-1.5 py-0.2 rounded bg-zinc-850 border border-zinc-750">
                  {selectedTunnel.id}
                </span>
              </div>
              <div className="text-[11px] text-zinc-400 mt-0.5">
                Evaluation Order: Rules are matched sequentially from top to bottom. The first match handles the traffic.
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <span className="text-zinc-400 text-[11px] font-mono">
              {rules.length} total rule{rules.length === 1 ? "" : "s"}
            </span>
          </div>
        </div>
      )}

      {/* Ingress Routing Rules Table */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900/40 overflow-hidden">
        <div className="px-4 py-3 bg-zinc-900/80 border-b border-zinc-800 flex items-center justify-between text-xs">
          <div className="font-semibold text-zinc-200 flex items-center gap-2">
            <GitBranch className="w-4 h-4 text-blue-400" />
            <span>Active Ingress Rules</span>
          </div>
          <span className="text-zinc-400 font-mono text-[11px]">
            Drag or use Priority buttons to reorder
          </span>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead className="bg-zinc-900/40 border-b border-zinc-850 text-zinc-400 font-semibold">
              <tr>
                <th className="py-2.5 px-4 w-28">Order</th>
                <th className="py-2.5 px-4">Hostname Pattern</th>
                <th className="py-2.5 px-4">Path Regex</th>
                <th className="py-2.5 px-4">Local Target Service</th>
                <th className="py-2.5 px-4">Flags</th>
                <th className="py-2.5 px-4 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-850">
              {rules.map((rule, idx) => {
                const isCatchAll = rule.service === "http_status:404";
                const canMoveUp = !isCatchAll && idx > 0;
                const canMoveDown = !isCatchAll && idx < maxMovableIndex;

                return (
                  <tr
                    key={idx}
                    className={`hover:bg-zinc-850/40 transition group ${
                      isCatchAll ? "bg-zinc-950/60 font-mono" : ""
                    }`}
                  >
                    {/* Priority & Reorder Controls */}
                    <td className="py-3 px-4">
                      {isCatchAll ? (
                        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded bg-zinc-850 text-zinc-400 border border-zinc-750 font-mono">
                          Catch-All
                        </span>
                      ) : (
                        <div className="flex items-center gap-1.5">
                          <span className="font-mono text-xs font-bold text-zinc-400 w-5">
                            #{idx + 1}
                          </span>
                          <div className="flex items-center">
                            <button
                              onClick={() => handleMoveUp(idx)}
                              disabled={!canMoveUp}
                              className="p-0.5 rounded text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 disabled:opacity-20 cursor-pointer disabled:cursor-default"
                              title="Move Up in Evaluation Order"
                            >
                              <ChevronUp className="w-3.5 h-3.5" />
                            </button>
                            <button
                              onClick={() => handleMoveDown(idx)}
                              disabled={!canMoveDown}
                              className="p-0.5 rounded text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 disabled:opacity-20 cursor-pointer disabled:cursor-default"
                              title="Move Down in Evaluation Order"
                            >
                              <ChevronDown className="w-3.5 h-3.5" />
                            </button>
                          </div>
                        </div>
                      )}
                    </td>

                    {/* Hostname */}
                    <td className="py-3 px-4">
                      {rule.hostname ? (
                        <span className="font-semibold text-zinc-200 font-mono">
                          {rule.hostname}
                        </span>
                      ) : (
                        <span className="text-zinc-500 italic">* (Wildcard / Any)</span>
                      )}
                    </td>

                    {/* Path */}
                    <td className="py-3 px-4 font-mono text-zinc-400">
                      {rule.path ? (
                        <span>{rule.path}</span>
                      ) : (
                        <span className="text-zinc-600 italic">/* (All paths)</span>
                      )}
                    </td>

                    {/* Service Target */}
                    <td className="py-3 px-4">
                      <div className="flex items-center gap-1.5 font-mono text-blue-300">
                        <ArrowRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                        <span className="truncate max-w-sm">{rule.service}</span>
                      </div>
                    </td>

                    {/* Flags */}
                    <td className="py-3 px-4">
                      <div className="flex items-center gap-1.5 flex-wrap">
                        {rule.origin_request?.no_tls_verify && (
                          <span className="inline-flex items-center gap-1 text-[10px] bg-amber-950/50 text-amber-300 border border-amber-800/40 px-1.5 py-0.5 rounded font-mono">
                            <AlertTriangle className="w-3 h-3" />
                            NoTLSVerify
                          </span>
                        )}
                        {rule.origin_request?.http_host_header && (
                          <span className="inline-flex items-center gap-1 text-[10px] bg-purple-950/50 text-purple-300 border border-purple-800/40 px-1.5 py-0.5 rounded font-mono">
                            Host: {rule.origin_request.http_host_header}
                          </span>
                        )}
                      </div>
                    </td>

                    {/* Actions */}
                    <td className="py-3 px-4 text-right">
                      {!isCatchAll && (
                        <button
                          onClick={() => setRuleToDeleteIndex(idx)}
                          className="p-1.5 rounded-lg text-zinc-500 hover:text-rose-400 hover:bg-rose-950/30 transition cursor-pointer"
                          title="Delete Ingress Rule"
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
                  <td colSpan={6} className="py-10 text-center text-zinc-500 italic">
                    {selectedTunnelId
                      ? "No ingress rules defined for this tunnel yet. Click \"Add Route\" to begin."
                      : "Please select a tunnel from the dropdown above."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Security & Architecture Note */}
      <div className="p-4 rounded-xl bg-zinc-950 border border-zinc-850 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-zinc-400">
        <div className="flex items-center gap-2.5">
          <Shield className="w-4 h-4 text-emerald-400 shrink-0" />
          <span>
            <strong>Zero Trust Ingress Invariant:</strong> Any incoming request not matched by your rules is safely dropped by the final catch-all <code className="text-zinc-300 font-mono">http_status:404</code> rule.
          </span>
        </div>
      </div>

      {/* Add Route Modal */}
      <Modal
        maxWidth="max-w-lg"
        isOpen={showAddModal}
        onClose={() => setShowAddModal(false)}
        title="Add Ingress Route"
        description="Direct incoming traffic from a hostname pattern to a local server port or service."
        icon={<Plus className="w-4 h-4 text-blue-400" />}
      >
        <div className="space-y-4">
          {/* Quick Framework / Service Presets */}
          <div className="space-y-1.5">
            <label className="text-[11px] font-semibold text-zinc-400 flex items-center gap-1">
              <Zap className="w-3.5 h-3.5 text-amber-400" />
              <span>Quick Service Presets</span>
            </label>
            <div className="flex flex-wrap gap-1.5">
              <button
                type="button"
                onClick={() => applyPreset("http", "localhost:3000")}
                className="px-2.5 py-1 rounded-lg bg-zinc-800/80 hover:bg-zinc-750 text-zinc-300 text-[11px] border border-zinc-700 transition cursor-pointer"
              >
                Next.js (:3000)
              </button>
              <button
                type="button"
                onClick={() => applyPreset("http", "localhost:5173")}
                className="px-2.5 py-1 rounded-lg bg-zinc-800/80 hover:bg-zinc-750 text-zinc-300 text-[11px] border border-zinc-700 transition cursor-pointer"
              >
                Vite (:5173)
              </button>
              <button
                type="button"
                onClick={() => applyPreset("http", "localhost:8000")}
                className="px-2.5 py-1 rounded-lg bg-zinc-800/80 hover:bg-zinc-750 text-zinc-300 text-[11px] border border-zinc-700 transition cursor-pointer"
              >
                FastAPI (:8000)
              </button>
              <button
                type="button"
                onClick={() => applyPreset("http", "localhost:8080")}
                className="px-2.5 py-1 rounded-lg bg-zinc-800/80 hover:bg-zinc-750 text-zinc-300 text-[11px] border border-zinc-700 transition cursor-pointer"
              >
                Go / Java (:8080)
              </button>
              <button
                type="button"
                onClick={() => applyPreset("ssh", "localhost:22")}
                className="px-2.5 py-1 rounded-lg bg-zinc-800/80 hover:bg-zinc-750 text-zinc-300 text-[11px] border border-zinc-700 transition cursor-pointer"
              >
                SSH (:22)
              </button>
            </div>
          </div>

          <form onSubmit={handleAddRule} className="space-y-4">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1">
                <label className="text-xs font-semibold text-zinc-300">Public Hostname</label>
                <input
                  type="text"
                  placeholder="e.g. api.yourdomain.com"
                  value={newHostname}
                  onChange={(e) => setNewHostname(e.target.value)}
                  className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                />
              </div>

              <div className="space-y-1">
                <label className="text-xs font-semibold text-zinc-300">Path (Optional Regex)</label>
                <input
                  type="text"
                  placeholder="e.g. /api/v1/*"
                  value={newPath}
                  onChange={(e) => setNewPath(e.target.value)}
                  className="w-full bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                />
              </div>
            </div>

            {/* Service Protocol and Target */}
            <div className="space-y-1">
              <label className="text-xs font-semibold text-zinc-300">Target Local Service</label>
              <div className="flex gap-2">
                <div className="w-36">
                  <SelectDropdown
                    options={protocolOptions}
                    value={newProtocol}
                    onChange={(val) => setNewProtocol(val)}
                    triggerClassName="h-9 px-2.5 py-0 bg-zinc-950 border-zinc-750 font-mono"
                    menuClassName="min-w-[200px]"
                  />
                </div>

                <input
                  type="text"
                  required
                  placeholder="localhost:3000"
                  value={newTarget}
                  onChange={(e) => setNewTarget(e.target.value)}
                  className="flex-1 bg-zinc-950 border border-zinc-750 rounded-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                />
              </div>
            </div>

            {/* Live Route Visualization Banner */}
            <div className="p-3.5 rounded-xl bg-zinc-950 border border-zinc-850 font-mono text-xs space-y-2">
              <div className="text-[10px] uppercase font-bold text-zinc-500 tracking-wider">
                Live Ingress Mapping Preview
              </div>
              <div className="flex items-center gap-2 text-zinc-300 overflow-x-auto py-1">
                <span className="text-blue-300 font-semibold truncate shrink-0">
                  {newHostname.trim() || "* (any host)"}
                  {newPath.trim() || ""}
                </span>
                <ArrowRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                <span className="text-emerald-300 font-semibold truncate shrink-0">
                  {newProtocol}://{newTarget.trim() || "localhost:3000"}
                </span>
              </div>
            </div>

            {/* Advanced Flags */}
            <div className="p-3.5 rounded-xl bg-zinc-950/70 border border-zinc-800 space-y-2.5">
              <div className="flex items-center justify-between text-xs">
                <div>
                  <span className="text-zinc-200 font-medium">Bypass Upstream TLS Verify</span>
                  <div className="text-[11px] text-zinc-500">
                    Enable if your local service uses self-signed HTTPS certificates.
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={noTlsVerify}
                  onChange={(e) => setNoTlsVerify(e.target.checked)}
                  className="rounded text-blue-600 focus:ring-0 cursor-pointer h-4 w-4"
                />
              </div>

              <div className="space-y-1 pt-1 border-t border-zinc-850">
                <label className="text-[11px] text-zinc-400">Custom HTTP Host Header (Override)</label>
                <input
                  type="text"
                  placeholder="e.g. internal.local"
                  value={hostHeader}
                  onChange={(e) => setHostHeader(e.target.value)}
                  className="w-full bg-zinc-900 border border-zinc-800 rounded-lg px-3 py-1.5 text-xs text-zinc-200 font-mono focus:outline-none"
                />
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setShowAddModal(false)}
                className="px-4 py-2 rounded-xl bg-zinc-800 hover:bg-zinc-750 text-zinc-300 text-xs font-medium transition cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer"
              >
                Add Route
              </button>
            </div>
          </form>
        </div>
      </Modal>

      {/* Confirm Delete Ingress Rule Modal */}
      <ConfirmModal
        isOpen={ruleToDeleteIndex !== null}
        title="Remove Ingress Route"
        description="Are you sure you want to remove this ingress route from your local configuration? (Remember to click 'Save to Edge' to deploy your updated configuration to Cloudflare)."
        details={
          ruleToDeleteIndex !== null && rules[ruleToDeleteIndex] && (
            <div className="space-y-1">
              <div className="font-semibold text-zinc-200">
                {rules[ruleToDeleteIndex].hostname || "* (Wildcard)"}
                {rules[ruleToDeleteIndex].path || ""}
              </div>
              <div className="text-zinc-500 font-mono text-[11px]">
                Target: {rules[ruleToDeleteIndex].service}
              </div>
            </div>
          )
        }
        confirmLabel="Remove Route"
        onClose={() => setRuleToDeleteIndex(null)}
        onConfirm={() => {
          if (ruleToDeleteIndex !== null) {
            handleDeleteRule(ruleToDeleteIndex);
            setRuleToDeleteIndex(null);
          }
        }}
      />
    </div>
  );
};
