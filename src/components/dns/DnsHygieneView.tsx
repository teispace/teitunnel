import React, { useEffect, useState } from "react";
import {
  ShieldCheck,
  Globe,
  Trash2,
  RefreshCw,
  AlertTriangle,
  Plus,
  Sparkles,
  ExternalLink,
  CheckCircle2,
} from "lucide-react";
import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { useDnsStore } from "@/stores/dns-store";

export const DnsHygieneView: React.FC = () => {
  const { activeAccountId, activeZoneId, zones } = useAuthStore();
  const { tunnels } = useTunnelStore();
  const {
    records,
    hygieneReport,
    isLoading,
    isScanning,
    isCleaning,
    fetchRecords,
    createCname,
    deleteRecord,
    scanHygiene,
    cleanupAllOrphaned,
  } = useDnsStore();

  const [showAddModal, setShowAddModal] = useState(false);
  const [newSubdomain, setNewSubdomain] = useState("");
  const [selectedTunnelUuid, setSelectedTunnelUuid] = useState(tunnels[0]?.id || "");
  const [cleanedCount, setCleanedCount] = useState<number | null>(null);

  const activeZone = zones.find((z) => z.id === activeZoneId);

  useEffect(() => {
    if (activeZoneId) {
      fetchRecords(activeZoneId);
    }
  }, [activeZoneId, fetchRecords]);

  const handleLinkDomain = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeZoneId || !newSubdomain.trim() || !selectedTunnelUuid) return;
    const fullName = newSubdomain.includes(".")
      ? newSubdomain
      : `${newSubdomain}.${activeZone?.name || ""}`;

    const success = await createCname(activeZoneId, fullName, selectedTunnelUuid);
    if (success) {
      setNewSubdomain("");
      setShowAddModal(false);
    }
  };

  const handleRunHygieneScan = async () => {
    if (!activeAccountId || !activeZoneId) return;
    setCleanedCount(null);
    await scanHygiene(activeAccountId, activeZoneId);
  };

  const handleBatchClean = async () => {
    if (!activeZoneId) return;
    if (confirm("Are you sure you want to delete all orphaned DNS records in this zone?")) {
      const count = await cleanupAllOrphaned(activeZoneId);
      setCleanedCount(count);
    }
  };

  const tunnelCnames = records.filter((r) => r.content.endsWith(".cfargotunnel.com"));

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <ShieldCheck className="w-5 h-5 text-emerald-400" />
            DNS & Hygiene Engine
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Automatic CNAME provisioning, zero dangling pointers, and 1-click orphaned record cleanup.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={() => activeZoneId && fetchRecords(activeZoneId)}
            disabled={isLoading}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Refresh DNS records"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={handleRunHygieneScan}
            disabled={isScanning}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-zinc-100 text-xs font-semibold border border-zinc-700 transition cursor-pointer disabled:opacity-60"
          >
            <Sparkles className={`w-4 h-4 text-amber-400 ${isScanning ? "animate-spin" : ""}`} />
            <span>{isScanning ? "Scanning Zone..." : "Scan Hygiene"}</span>
          </button>

          <button
            onClick={() => setShowAddModal(true)}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>Link Domain</span>
          </button>
        </div>
      </div>

      {/* Hygiene Alert Card (if scan was run and found orphans) */}
      {hygieneReport && hygieneReport.orphaned_records.length > 0 && (
        <div className="p-5 rounded-2xl bg-amber-950/40 border border-amber-500/50 shadow-xl space-y-4">
          <div className="flex items-start justify-between gap-4">
            <div className="flex items-start gap-3">
              <div className="w-9 h-9 rounded-xl bg-amber-500/20 border border-amber-500/40 flex items-center justify-center text-amber-400 shrink-0">
                <AlertTriangle className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-amber-200">
                  {hygieneReport.orphaned_records.length} Orphaned Tunnel DNS Records Detected
                </h3>
                <p className="text-xs text-amber-300/80 mt-1 leading-relaxed">
                  These CNAME records point to deleted or non-existent tunnel UUIDs. Leaving them creates potential subdomain takeover risks and clutters your Cloudflare zone.
                </p>
              </div>
            </div>

            <button
              onClick={handleBatchClean}
              disabled={isCleaning}
              className="px-4 py-2 rounded-xl bg-amber-600 hover:bg-amber-500 text-white text-xs font-semibold shadow-lg transition cursor-pointer shrink-0 disabled:opacity-60"
            >
              {isCleaning ? "Purging Records..." : "1-Click Clean Up"}
            </button>
          </div>

          <div className="p-3 bg-zinc-950/80 rounded-xl border border-amber-900/60 font-mono text-xs space-y-1.5">
            {hygieneReport.orphaned_records.map((item, idx) => (
              <div key={idx} className="flex items-center justify-between text-zinc-300">
                <span>{item.record.name}</span>
                <span className="text-zinc-500 truncate max-w-xs">{item.record.content}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Cleaned Confirmation */}
      {cleanedCount !== null && (
        <div className="p-4 rounded-xl bg-emerald-950/40 border border-emerald-800/60 flex items-center gap-2.5 text-xs text-emerald-300">
          <CheckCircle2 className="w-4 h-4 text-emerald-400" />
          <span>Successfully purged {cleanedCount} orphaned DNS records from Cloudflare DNS!</span>
        </div>
      )}

      {/* Tunnel CNAME Records Table */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900/40 overflow-hidden space-y-0">
        <div className="px-4 py-3 bg-zinc-900/80 border-b border-zinc-800 flex items-center justify-between text-xs">
          <div className="font-semibold text-zinc-200 flex items-center gap-2">
            <Globe className="w-4 h-4 text-blue-400" />
            <span>Active Tunnel Hostnames ({activeZone?.name || "Select Zone"})</span>
          </div>
          <span className="text-zinc-400 font-mono">{tunnelCnames.length} CNAME records</span>
        </div>

        <table className="w-full text-left text-xs">
          <thead className="bg-zinc-900/40 border-b border-zinc-850 text-zinc-400 font-semibold">
            <tr>
              <th className="py-2.5 px-4">Hostname</th>
              <th className="py-2.5 px-4">Type</th>
              <th className="py-2.5 px-4">Target Tunnel Target</th>
              <th className="py-2.5 px-4">Proxy Status</th>
              <th className="py-2.5 px-4 text-right">Delete</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-850">
            {tunnelCnames.map((record) => (
              <tr key={record.id} className="hover:bg-zinc-850/40 transition">
                <td className="py-3 px-4 font-semibold text-zinc-200">
                  <a
                    href={`https://${record.name}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="hover:text-blue-400 flex items-center gap-1.5"
                  >
                    <span>{record.name}</span>
                    <ExternalLink className="w-3 h-3 text-zinc-500" />
                  </a>
                </td>
                <td className="py-3 px-4 font-mono text-zinc-400">CNAME</td>
                <td className="py-3 px-4 font-mono text-zinc-400 text-[11px] truncate max-w-sm">
                  {record.content}
                </td>
                <td className="py-3 px-4">
                  <span className="text-[10px] font-medium bg-orange-950/50 text-orange-400 border border-orange-800/40 px-2 py-0.5 rounded-full">
                    Proxied (Cloudflare CDN)
                  </span>
                </td>
                <td className="py-3 px-4 text-right">
                  <button
                    onClick={() => {
                      if (confirm(`Delete DNS record "${record.name}"?`)) {
                        if (activeZoneId) deleteRecord(activeZoneId, record.id);
                      }
                    }}
                    className="p-1 rounded text-zinc-500 hover:text-rose-400 transition cursor-pointer"
                    title="Delete DNS Record"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </td>
              </tr>
            ))}

            {tunnelCnames.length === 0 && (
              <tr>
                <td colSpan={5} className="py-8 text-center text-zinc-500 italic">
                  No tunnel CNAME records in this zone yet. Click &quot;Link Domain&quot; to connect a hostname.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* Link Domain Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="w-full max-w-md p-6 rounded-2xl bg-zinc-900 border border-zinc-800 shadow-2xl space-y-4">
            <h3 className="text-base font-bold text-zinc-100 flex items-center gap-2">
              <Plus className="w-4 h-4 text-blue-400" />
              Link Hostname to Tunnel
            </h3>
            <p className="text-xs text-zinc-400">
              Provisions a proxied CNAME DNS record pointing to your tunnel endpoint on Cloudflare.
            </p>

            <form onSubmit={handleLinkDomain} className="space-y-4">
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-zinc-300">Subdomain / Hostname</label>
                <div className="flex items-center">
                  <input
                    type="text"
                    required
                    placeholder="e.g. api or app"
                    value={newSubdomain}
                    onChange={(e) => setNewSubdomain(e.target.value)}
                    className="flex-1 bg-zinc-950 border border-zinc-750 rounded-l-lg px-3.5 py-2 text-xs text-zinc-100 focus:border-blue-500 focus:outline-none font-mono"
                  />
                  <div className="bg-zinc-800 border-y border-r border-zinc-750 rounded-r-lg px-3 py-2 text-xs text-zinc-400 font-mono">
                    .{activeZone?.name || "domain.com"}
                  </div>
                </div>
              </div>

              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-zinc-300">Target Tunnel</label>
                <select
                  value={selectedTunnelUuid}
                  onChange={(e) => setSelectedTunnelUuid(e.target.value)}
                  className="w-full bg-zinc-950 border border-zinc-750 rounded-lg px-3 py-2 text-xs text-zinc-100 focus:outline-none cursor-pointer"
                >
                  {tunnels.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name} ({t.id.slice(0, 8)}...)
                    </option>
                  ))}
                </select>
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
                  Provision DNS Record
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
