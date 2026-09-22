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
  Copy,
  Check,
  Search,
  ArrowRight,
  ShieldAlert,
  Server,
  Filter,
} from "lucide-react";
import { toast } from "sonner";
import { useAuthStore } from "@/stores/auth-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { useDnsStore } from "@/stores/dns-store";
import { SelectDropdown, type SelectOption } from "@/components/ui/SelectDropdown";
import { ConfirmModal } from "@/components/ui/ConfirmModal";
import { Modal } from "@/components/ui/Modal";
import type { DnsRecord } from "@/lib/tauri";

export const DnsHygieneView: React.FC = () => {
  const { activeAccountId, activeZoneId, zones, token, certStatus, selectZone } = useAuthStore();
  const { tunnels, fetchCertTunnels, fetchTunnels } = useTunnelStore();
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
  const [searchQuery, setSearchQuery] = useState("");
  const [filterMode, setFilterMode] = useState<"tunnel" | "all">("tunnel");
  const [copiedText, setCopiedText] = useState<string | null>(null);

  // Native Confirm Dialog states
  const [recordToDelete, setRecordToDelete] = useState<DnsRecord | null>(null);
  const [isDeletingRecord, setIsDeletingRecord] = useState(false);
  const [showBatchCleanModal, setShowBatchCleanModal] = useState(false);
  const [isBatchCleaning, setIsBatchCleaning] = useState(false);

  // Auto-select first zone if available and none selected
  useEffect(() => {
    if (!activeZoneId && zones.length > 0) {
      selectZone(zones[0].id);
    }
  }, [activeZoneId, zones, selectZone]);

  const activeZone = zones.find((z) => z.id === activeZoneId) || zones[0];

  useEffect(() => {
    if (activeZone?.id) {
      fetchRecords(activeZone.id);
    }
  }, [activeZone?.id, fetchRecords]);

  // Ensure tunnels are populated for target selection
  useEffect(() => {
    if (tunnels.length === 0) {
      if (activeAccountId && token) {
        fetchTunnels(activeAccountId, token);
      } else if (certStatus?.has_cert) {
        fetchCertTunnels();
      }
    }
  }, [tunnels.length, activeAccountId, token, certStatus?.has_cert, fetchTunnels, fetchCertTunnels]);

  // Keep selected tunnel in sync if tunnels load
  useEffect(() => {
    if (!selectedTunnelUuid && tunnels.length > 0) {
      setSelectedTunnelUuid(tunnels[0].id);
    }
  }, [tunnels, selectedTunnelUuid]);

  const copyToClipboard = (text: string, label: string) => {
    navigator.clipboard.writeText(text);
    setCopiedText(text);
    toast.success(`Copied ${label} to clipboard`);
    setTimeout(() => setCopiedText(null), 2000);
  };

  const handleLinkDomain = async (e: React.FormEvent) => {
    e.preventDefault();
    const targetZoneId = activeZone?.id || activeZoneId;
    if (!targetZoneId) {
      toast.error("Please select a Cloudflare Zone first.");
      return;
    }
    if (!newSubdomain.trim() || !selectedTunnelUuid) {
      toast.error("Please specify both a subdomain and a target tunnel.");
      return;
    }

    const trimmed = newSubdomain.trim().toLowerCase();
    const fullName = trimmed.includes(".")
      ? trimmed
      : `${trimmed}.${activeZone?.name || ""}`;

    const toastId = toast.loading(`Provisioning DNS CNAME for ${fullName}...`);
    const ok = await createCname(targetZoneId, fullName, selectedTunnelUuid, token || undefined);
    if (ok) {
      toast.success(`CNAME ${fullName} provisioned successfully!`, { id: toastId });
      setNewSubdomain("");
      setShowAddModal(false);
    } else {
      const errMsg = useDnsStore.getState().error || "Failed to create DNS CNAME record.";
      toast.error(`DNS creation failed: ${errMsg}`, { id: toastId });
    }
  };

  const handleRunHygieneScan = async () => {
    const targetZoneId = activeZone?.id || activeZoneId;
    const targetAccountId = activeAccountId || (zones.length > 0 ? zones[0].id : null);
    if (!targetAccountId || !targetZoneId) {
      toast.error("Please select an active Account and Zone before scanning.");
      return;
    }
    setCleanedCount(null);
    toast.info(`Scanning zone "${activeZone?.name || "Cloudflare"}" for orphaned CNAMEs...`);
    await scanHygiene(targetAccountId, targetZoneId);
    const report = useDnsStore.getState().hygieneReport;
    if (report) {
      if (report.orphaned_records.length === 0) {
        toast.success("Zone is clean! Zero orphaned DNS records detected.");
      } else {
        toast.warning(
          `Hygiene Alert: ${report.orphaned_records.length} orphaned DNS records detected!`
        );
      }
    }
  };

  const tunnelCnames = records.filter((r) => r.content.endsWith(".cfargotunnel.com"));

  const filteredRecords = records.filter((r) => {
    const matchesSearch =
      !searchQuery.trim() ||
      r.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      r.content.toLowerCase().includes(searchQuery.toLowerCase()) ||
      r.type.toLowerCase().includes(searchQuery.toLowerCase());

    if (!matchesSearch) return false;

    if (filterMode === "tunnel") {
      return r.content.endsWith(".cfargotunnel.com");
    }
    return true;
  });

  const tunnelOptions: SelectOption[] = tunnels.map((t) => ({
    value: t.id,
    label: t.name,
    sublabel: `UUID: ${t.id.slice(0, 8)}...`,
    badge: t.status === "healthy" ? "Online" : t.status || "Tunnel",
    badgeColor:
      t.status === "healthy"
        ? "bg-emerald-950/60 text-emerald-300 border-emerald-800/40"
        : "bg-zinc-800 text-zinc-400 border-zinc-700",
    icon: <Server className="w-3.5 h-3.5 text-blue-400" />,
  }));

  const orphanedCount = hygieneReport?.orphaned_records.length || 0;

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <ShieldCheck className="w-5 h-5 text-emerald-400" />
            <span>DNS &amp; Hygiene Engine</span>
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Automatic CNAME provisioning, zero dangling pointers, and 1-click orphaned record cleanup.
          </p>
        </div>

        <div className="flex items-center gap-2 flex-wrap">
          {/* Inline Zone Selector */}
          {zones.length > 0 && (
            <div className="flex items-center gap-1.5">
              <span className="text-xs text-zinc-400 font-medium hidden md:inline">Zone:</span>
              <SelectDropdown
                options={zones.map((z) => ({
                  value: z.id,
                  label: z.name,
                  sublabel: z.status,
                  badge: z.status === "active" ? "Active" : z.status,
                  badgeColor: "bg-emerald-950/60 text-emerald-300 border-emerald-800/40",
                  icon: <Globe className="w-3.5 h-3.5 text-blue-400" />,
                }))}
                value={activeZone?.id || ""}
                onChange={(val) => selectZone(val)}
                searchable={zones.length > 3}
                placeholder="Select Zone..."
                triggerClassName="h-9 px-3 bg-zinc-900 border-zinc-750 text-xs text-zinc-200"
                menuClassName="min-w-[220px]"
              />
            </div>
          )}

          <button
            onClick={() => {
              if (activeZone?.id) {
                fetchRecords(activeZone.id);
                toast.info(`Refreshed DNS records for ${activeZone.name}`);
              } else {
                toast.error("Please select or configure a Zone first.");
              }
            }}
            disabled={isLoading}
            className="p-2 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-zinc-300 transition cursor-pointer"
            title="Refresh DNS records"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? "animate-spin text-blue-400" : ""}`} />
          </button>

          <button
            onClick={handleRunHygieneScan}
            disabled={isScanning || !activeZone?.id}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-750 text-zinc-100 text-xs font-semibold border border-zinc-700 transition cursor-pointer disabled:opacity-50"
          >
            <Sparkles className={`w-4 h-4 text-amber-400 ${isScanning ? "animate-spin" : ""}`} />
            <span>{isScanning ? "Scanning Zone..." : "Scan Hygiene"}</span>
          </button>

          <button
            onClick={() => {
              if (!activeZone?.id) {
                toast.error("Please select a Cloudflare Zone first.");
                return;
              }
              setShowAddModal(true);
            }}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md shadow-blue-950/40 transition cursor-pointer"
          >
            <Plus className="w-4 h-4" />
            <span>Link Domain</span>
          </button>
        </div>
      </div>

      {/* No API Token / Zone Warning */}
      {!token && (
        <div className="p-4 rounded-xl bg-blue-950/30 border border-blue-800/50 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs">
          <div className="flex items-center gap-3">
            <Globe className="w-5 h-5 text-blue-400 shrink-0" />
            <div>
              <div className="font-semibold text-blue-200">
                DNS Management requires an active Cloudflare API Token
              </div>
              <div className="text-zinc-400 text-[11px] mt-0.5">
                Configure your scoped API Token in Settings to manage CNAME records and run the hygiene cleaner.
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Hygiene Engine Summary Stats Bar */}
      {activeZone && (
        <div className="grid grid-cols-1 sm:grid-cols-4 gap-3">
          <div className="p-3.5 rounded-xl bg-zinc-900/60 border border-zinc-800 flex items-center justify-between">
            <div>
              <div className="text-[11px] text-zinc-400 font-medium">Selected Zone</div>
              <div className="text-xs font-bold text-zinc-200 truncate mt-0.5">{activeZone.name}</div>
            </div>
            <div className="w-7 h-7 rounded-lg bg-blue-950/50 border border-blue-800/40 flex items-center justify-center text-blue-400">
              <Globe className="w-3.5 h-3.5" />
            </div>
          </div>

          <div className="p-3.5 rounded-xl bg-zinc-900/60 border border-zinc-800 flex items-center justify-between">
            <div>
              <div className="text-[11px] text-zinc-400 font-medium">Tunnel CNAMEs</div>
              <div className="text-xs font-bold text-zinc-200 font-mono mt-0.5">
                {tunnelCnames.length}
              </div>
            </div>
            <div className="w-7 h-7 rounded-lg bg-emerald-950/50 border border-emerald-800/40 flex items-center justify-center text-emerald-400">
              <ShieldCheck className="w-3.5 h-3.5" />
            </div>
          </div>

          <div className="p-3.5 rounded-xl bg-zinc-900/60 border border-zinc-800 flex items-center justify-between">
            <div>
              <div className="text-[11px] text-zinc-400 font-medium">Orphaned Records</div>
              <div
                className={`text-xs font-bold font-mono mt-0.5 ${
                  orphanedCount > 0 ? "text-amber-400" : "text-emerald-400"
                }`}
              >
                {orphanedCount}
              </div>
            </div>
            <div
              className={`w-7 h-7 rounded-lg flex items-center justify-center ${
                orphanedCount > 0
                  ? "bg-amber-950/50 border border-amber-800/40 text-amber-400"
                  : "bg-emerald-950/50 border border-emerald-800/40 text-emerald-400"
              }`}
            >
              <AlertTriangle className="w-3.5 h-3.5" />
            </div>
          </div>

          <div className="p-3.5 rounded-xl bg-zinc-900/60 border border-zinc-800 flex items-center justify-between">
            <div>
              <div className="text-[11px] text-zinc-400 font-medium">Security Status</div>
              <div className="text-xs font-bold mt-0.5">
                {orphanedCount > 0 ? (
                  <span className="text-amber-400 flex items-center gap-1">
                    <ShieldAlert className="w-3 h-3" /> Dangling Risk
                  </span>
                ) : (
                  <span className="text-emerald-400 flex items-center gap-1">
                    <CheckCircle2 className="w-3 h-3" /> Protected
                  </span>
                )}
              </div>
            </div>
            <div className="w-7 h-7 rounded-lg bg-zinc-850 border border-zinc-750 flex items-center justify-center text-zinc-400">
              <ShieldCheck className="w-3.5 h-3.5" />
            </div>
          </div>
        </div>
      )}

      {/* Hygiene Alert Card (if scan detected orphaned records) */}
      {hygieneReport && hygieneReport.orphaned_records.length > 0 && (
        <div className="p-5 rounded-2xl bg-amber-950/35 border border-amber-500/50 shadow-xl space-y-4 animate-in fade-in">
          <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4">
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
              onClick={() => setShowBatchCleanModal(true)}
              disabled={isCleaning}
              className="px-4 py-2 rounded-xl bg-amber-600 hover:bg-amber-500 text-white text-xs font-semibold shadow-lg transition cursor-pointer shrink-0 disabled:opacity-60"
            >
              {isCleaning ? "Purging Records..." : `1-Click Clean Up (${hygieneReport.orphaned_records.length})`}
            </button>
          </div>

          <div className="p-3 bg-zinc-950/80 rounded-xl border border-amber-900/60 font-mono text-xs space-y-2">
            {hygieneReport.orphaned_records.map((item, idx) => (
              <div key={idx} className="flex items-center justify-between text-zinc-300 gap-2">
                <span className="font-semibold text-amber-200 truncate">{item.record.name}</span>
                <span className="text-zinc-500 truncate max-w-xs">{item.record.content}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Cleaned Confirmation */}
      {cleanedCount !== null && (
        <div className="p-4 rounded-xl bg-emerald-950/40 border border-emerald-800/60 flex items-center gap-2.5 text-xs text-emerald-300">
          <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
          <span>Successfully purged {cleanedCount} orphaned DNS record(s) from Cloudflare DNS!</span>
        </div>
      )}

      {/* Search & Filter Toolbar */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div className="relative flex-1 max-w-md">
          <Search className="w-3.5 h-3.5 text-zinc-500 absolute left-3 top-2.5" />
          <input
            type="text"
            placeholder="Search hostnames, targets, or record types..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full bg-zinc-900 border border-zinc-800 rounded-xl pl-9 pr-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:outline-none"
          />
        </div>

        <div className="flex items-center gap-1 bg-zinc-950 p-1 rounded-xl border border-zinc-800 text-xs self-start sm:self-auto">
          <button
            onClick={() => setFilterMode("tunnel")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
              filterMode === "tunnel"
                ? "bg-blue-600 text-white shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>Tunnel CNAMEs</span>
            <span className="text-[10px] font-mono px-1 rounded bg-black/30">
              {tunnelCnames.length}
            </span>
          </button>

          <button
            onClick={() => setFilterMode("all")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
              filterMode === "all"
                ? "bg-blue-600 text-white shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <Filter className="w-3.5 h-3.5" />
            <span>All Records</span>
            <span className="text-[10px] font-mono px-1 rounded bg-black/30">
              {records.length}
            </span>
          </button>
        </div>
      </div>

      {/* DNS Records Table */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900/40 overflow-hidden space-y-0">
        <div className="px-4 py-3 bg-zinc-900/80 border-b border-zinc-800 flex items-center justify-between text-xs">
          <div className="font-semibold text-zinc-200 flex items-center gap-2">
            <Globe className="w-4 h-4 text-blue-400" />
            <span>
              DNS Records ({activeZone ? activeZone.name : "Select Zone in Top Bar"})
            </span>
          </div>
          <span className="text-zinc-400 font-mono">
            {filteredRecords.length} record{filteredRecords.length === 1 ? "" : "s"} shown
          </span>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead className="bg-zinc-900/40 border-b border-zinc-850 text-zinc-400 font-semibold">
              <tr>
                <th className="py-2.5 px-4">Hostname</th>
                <th className="py-2.5 px-4">Type</th>
                <th className="py-2.5 px-4">Target / Content</th>
                <th className="py-2.5 px-4">Proxy Status</th>
                <th className="py-2.5 px-4 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-850">
              {filteredRecords.map((record) => {
                const isTunnelCname = record.content.endsWith(".cfargotunnel.com");
                return (
                  <tr key={record.id} className="hover:bg-zinc-850/40 transition">
                    <td className="py-3 px-4 font-semibold text-zinc-200">
                      <div className="flex items-center gap-2">
                        <a
                          href={`https://${record.name}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="hover:text-blue-400 flex items-center gap-1.5 truncate max-w-xs"
                          title="Open in Browser"
                        >
                          <span className="truncate">{record.name}</span>
                          <ExternalLink className="w-3 h-3 text-zinc-500 shrink-0" />
                        </a>
                        <button
                          onClick={() => copyToClipboard(record.name, "Hostname")}
                          className="p-1 rounded text-zinc-500 hover:text-zinc-300 transition cursor-pointer"
                          title="Copy hostname"
                        >
                          {copiedText === record.name ? (
                            <Check className="w-3 h-3 text-emerald-400" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                    </td>
                    <td className="py-3 px-4 font-mono text-zinc-400">
                      <div className="flex items-center gap-1.5">
                        <span className="px-1.5 py-0.5 rounded bg-zinc-850 border border-zinc-750 font-bold text-[10px]">
                          {record.type}
                        </span>
                        {isTunnelCname && (
                          <span className="px-1.5 py-0.5 rounded bg-blue-950/60 border border-blue-800/40 text-blue-300 font-sans font-semibold text-[9px]">
                            Tunnel
                          </span>
                        )}
                      </div>
                    </td>
                    <td className="py-3 px-4 font-mono text-zinc-300 text-[11px]">
                      <div className="flex items-center gap-2 truncate max-w-sm">
                        <span className="truncate">{record.content}</span>
                        <button
                          onClick={() => copyToClipboard(record.content, "Target Content")}
                          className="p-1 rounded text-zinc-500 hover:text-zinc-300 transition cursor-pointer shrink-0"
                          title="Copy target"
                        >
                          {copiedText === record.content ? (
                            <Check className="w-3 h-3 text-emerald-400" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                    </td>
                    <td className="py-3 px-4">
                      {record.proxied ? (
                        <span className="text-[10px] font-medium bg-orange-950/50 text-orange-400 border border-orange-800/40 px-2 py-0.5 rounded-full inline-flex items-center gap-1">
                          <span className="w-1.5 h-1.5 rounded-full bg-orange-400" />
                          Proxied (Cloudflare CDN)
                        </span>
                      ) : (
                        <span className="text-[10px] font-medium bg-zinc-800 text-zinc-400 border border-zinc-700 px-2 py-0.5 rounded-full inline-flex items-center gap-1">
                          DNS Only
                        </span>
                      )}
                    </td>
                    <td className="py-3 px-4 text-right">
                      <button
                        onClick={() => setRecordToDelete(record)}
                        className="p-1.5 rounded-lg text-zinc-500 hover:text-rose-400 hover:bg-rose-950/30 transition cursor-pointer"
                        title={`Delete ${record.name}`}
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </td>
                  </tr>
                );
              })}

              {filteredRecords.length === 0 && (
                <tr>
                  <td colSpan={5} className="py-10 text-center text-zinc-500 italic">
                    {searchQuery
                      ? "No records matching your search."
                      : !activeZoneId
                      ? "Select a Cloudflare Zone from the top bar to view DNS records."
                      : "No records found in this zone. Click \"Link Domain\" to add a tunnel hostname."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Link Domain Modal with SelectDropdown & Live Preview */}
      <Modal
        maxWidth="max-w-lg"
        isOpen={showAddModal}
        onClose={() => setShowAddModal(false)}
        title="Link Hostname to Tunnel"
        description="Provisions an automated CNAME record with Cloudflare edge proxying enabled."
        icon={<Plus className="w-4 h-4 text-blue-400" />}
      >
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
                className="flex-1 bg-zinc-950 border border-zinc-750 rounded-l-xl px-3.5 py-2 text-xs text-zinc-100 placeholder-zinc-500 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
              />
              <div className="bg-zinc-800 border-y border-r border-zinc-750 rounded-r-xl px-3.5 py-2 text-xs text-zinc-400 font-mono">
                .{activeZone?.name || "yourdomain.com"}
              </div>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-zinc-300">Target Tunnel</label>
            {tunnels.length > 0 ? (
              <SelectDropdown
                options={tunnelOptions}
                value={selectedTunnelUuid}
                onChange={(val) => setSelectedTunnelUuid(val)}
                searchable={tunnels.length > 3}
                placeholder="Select target tunnel..."
                triggerClassName="w-full bg-zinc-950 border-zinc-750"
                menuClassName="w-full"
              />
            ) : (
              <div className="p-3 rounded-xl bg-zinc-950 border border-zinc-800 text-xs text-zinc-400">
                No active tunnels found. Create or start a tunnel first in the Tunnels tab.
              </div>
            )}
          </div>

          {/* Live Route Visualization Banner */}
          <div className="p-3.5 rounded-xl bg-zinc-950 border border-zinc-850 font-mono text-xs space-y-2">
            <div className="text-[10px] uppercase font-bold text-zinc-500 tracking-wider">
              Live DNS Route Preview
            </div>
            <div className="flex items-center gap-2 text-zinc-300 overflow-x-auto py-1">
              <span className="text-blue-300 font-semibold truncate shrink-0">
                {newSubdomain.trim()
                  ? newSubdomain.includes(".")
                    ? newSubdomain.trim()
                    : `${newSubdomain.trim()}.${activeZone?.name || "domain.com"}`
                  : `api.${activeZone?.name || "domain.com"}`}
              </span>
              <ArrowRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
              <span className="text-orange-400 text-[10px] px-2 py-0.5 rounded-full bg-orange-950/60 border border-orange-800/40 shrink-0">
                Cloudflare Proxy
              </span>
              <ArrowRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
              <span className="text-emerald-400 text-[11px] truncate shrink-0">
                {selectedTunnelUuid
                  ? `${selectedTunnelUuid.slice(0, 8)}...cfargotunnel.com`
                  : "tunnel.cfargotunnel.com"}
              </span>
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
              disabled={!selectedTunnelUuid || !newSubdomain.trim()}
              className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-md transition cursor-pointer disabled:opacity-50"
            >
              Provision DNS Record
            </button>
          </div>
        </form>
      </Modal>

      {/* Native Confirm Modal: Delete Single DNS Record */}
      <ConfirmModal
        isOpen={Boolean(recordToDelete)}
        title="Delete DNS Record"
        description="Are you sure you want to delete this DNS record? Inbound traffic addressed to this hostname will no longer be routed to your tunnel."
        details={
          recordToDelete && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-zinc-100">{recordToDelete.name}</span>
                <span className="text-[10px] uppercase font-bold px-2 py-0.5 rounded bg-zinc-800 text-zinc-300">
                  {recordToDelete.type}
                </span>
              </div>
              <div className="text-[11px] text-zinc-500 font-mono select-all">
                Points to: {recordToDelete.content}
              </div>
            </div>
          )
        }
        confirmLabel="Delete Record"
        isLoading={isDeletingRecord}
        onClose={() => !isDeletingRecord && setRecordToDelete(null)}
        onConfirm={async () => {
          if (!recordToDelete) return;
          setIsDeletingRecord(true);
          const toastId = toast.loading(`Deleting DNS record "${recordToDelete.name}"...`);
          try {
            const targetZoneId = activeZone?.id || activeZoneId;
            if (!targetZoneId) return;
            const ok = await deleteRecord(targetZoneId, recordToDelete.id, token || undefined);
            if (ok) {
              toast.success(`Deleted DNS record "${recordToDelete.name}"`, { id: toastId });
              setRecordToDelete(null);
            } else {
              const errMsg = useDnsStore.getState().error || "Failed to delete record";
              toast.error(`Delete failed: ${errMsg}`, { id: toastId });
            }
          } finally {
            setIsDeletingRecord(false);
          }
        }}
      />

      {/* Native Confirm Modal: Batch Clean Orphaned Records */}
      <ConfirmModal
        isOpen={showBatchCleanModal}
        title="Clean All Orphaned DNS Records"
        description={`This will purge all ${hygieneReport?.orphaned_records.length || 0} orphaned CNAME records pointing to deleted or inactive Cloudflare tunnels, protecting your domain from dangling CNAME takeovers.`}
        confirmLabel="Purge All Orphaned"
        isLoading={isBatchCleaning}
        onClose={() => !isBatchCleaning && setShowBatchCleanModal(false)}
        onConfirm={async () => {
          setIsBatchCleaning(true);
          const toastId = toast.loading("Purging orphaned DNS records...");
          try {
            const targetZoneId = activeZone?.id || activeZoneId;
            if (!targetZoneId) return;
            const count = await cleanupAllOrphaned(targetZoneId, token || undefined);
            setCleanedCount(count);
            if (count > 0) {
              toast.success(`Cleaned up ${count} orphaned DNS records!`, { id: toastId });
              setShowBatchCleanModal(false);
            } else {
              toast.info("No orphaned records were deleted.", { id: toastId });
            }
          } finally {
            setIsBatchCleaning(false);
          }
        }}
      />
    </div>
  );
};
