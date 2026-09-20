import { useState, useEffect } from "react";
import {
  Activity,
  Zap,
  Globe2,
  Server,
  TrendingUp,
  Clock,
} from "lucide-react";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { useTunnelStore } from "@/stores/tunnel-store";
import { tauriApi, type TunnelMetrics, type CloudflareTunnel } from "@/lib/tauri";

interface MetricsViewProps {
  initialTunnel?: CloudflareTunnel | null;
}

interface LatencyPoint {
  time: string;
  rtt: number;
}

export const MetricsView: React.FC<MetricsViewProps> = ({ initialTunnel }) => {
  const { tunnels, activeProcesses } = useTunnelStore();
  const [selectedTunnelId, setSelectedTunnelId] = useState<string>(
    initialTunnel?.id || Object.keys(activeProcesses)[0] || tunnels[0]?.id || ""
  );

  const [metrics, setMetrics] = useState<TunnelMetrics | null>(null);
  const [latencyHistory, setLatencyHistory] = useState<LatencyPoint[]>([]);
  const [isPolling, setIsPolling] = useState(true);

  const activeProcess = activeProcesses[selectedTunnelId];

  useEffect(() => {
    let interval: ReturnType<typeof setInterval>;


    const poll = async () => {
      if (!selectedTunnelId || !activeProcess?.metrics_port) return;
      try {
        const data = await tauriApi.getTunnelMetrics(
          selectedTunnelId,
          activeProcess.metrics_port
        );
        setMetrics(data);

        setLatencyHistory((prev) => {
          const next = [
            ...prev,
            {
              time: new Date().toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
                second: "2-digit",
              }),
              rtt: Number(data.avg_rtt_ms.toFixed(1)),
            },
          ];
          if (next.length > 25) next.shift();
          return next;
        });
      } catch (e) {
        console.error("Failed to poll metrics:", e);
      }
    };

    if (isPolling && activeProcess?.metrics_port) {
      poll();
      interval = setInterval(poll, 3000);
    }

    return () => {
      if (interval) clearInterval(interval);
    };
  }, [selectedTunnelId, activeProcess, isPolling]);

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-zinc-100 flex items-center gap-2">
            <Activity className="w-5 h-5 text-purple-400" />
            Telemetry & Edge Colos
          </h1>
          <p className="text-xs text-zinc-400 mt-0.5">
            Real-time latency, Prometheus metrics, and active Cloudflare Edge data centers.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <select
            value={selectedTunnelId}
            onChange={(e) => setSelectedTunnelId(e.target.value)}
            className="bg-zinc-900 border border-zinc-750 text-xs text-zinc-200 px-3 py-2 rounded-lg focus:outline-none cursor-pointer"
          >
            {tunnels.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name} {activeProcesses[t.id] ? "(Running)" : "(Stopped)"}
              </option>
            ))}
          </select>

          <button
            onClick={() => setIsPolling(!isPolling)}
            className={`px-3 py-2 rounded-lg text-xs font-medium border transition cursor-pointer ${
              isPolling
                ? "bg-purple-950/40 border-purple-800 text-purple-300"
                : "bg-zinc-900 border-zinc-800 text-zinc-400"
            }`}
          >
            {isPolling ? "Live Polling" : "Paused"}
          </button>
        </div>
      </div>

      {!activeProcess ? (
        <div className="p-8 rounded-2xl bg-zinc-950 border border-zinc-850 text-center space-y-2">
          <div className="w-10 h-10 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center text-zinc-400 mx-auto">
            <Server className="w-5 h-5" />
          </div>
          <div className="text-sm font-semibold text-zinc-200">Tunnel is not running locally</div>
          <p className="text-xs text-zinc-500 max-w-sm mx-auto">
            Start this tunnel from the &quot;Zero Trust Tunnels&quot; tab to begin scraping real-time Prometheus telemetry.
          </p>
        </div>
      ) : (
        <>
          {/* Key Metrics Cards */}
          <div className="grid grid-cols-1 sm:grid-cols-4 gap-4">
            <div className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 space-y-1">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span>Avg Latency (RTT)</span>
                <Clock className="w-4 h-4 text-blue-400" />
              </div>
              <div className="text-2xl font-bold font-mono text-zinc-100">
                {metrics?.avg_rtt_ms ? `${metrics.avg_rtt_ms.toFixed(1)} ms` : "--"}
              </div>
              <div className="text-[11px] text-zinc-500">Round-trip to edge</div>
            </div>

            <div className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 space-y-1">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span>Edge Connections</span>
                <Globe2 className="w-4 h-4 text-emerald-400" />
              </div>
              <div className="text-2xl font-bold font-mono text-zinc-100">
                {metrics?.active_connections || 0}
              </div>
              <div className="text-[11px] text-zinc-500">Active QUIC / HTTP2 links</div>
            </div>

            <div className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 space-y-1">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span>Total Requests</span>
                <TrendingUp className="w-4 h-4 text-purple-400" />
              </div>
              <div className="text-2xl font-bold font-mono text-zinc-100">
                {metrics?.total_requests || 0}
              </div>
              <div className="text-[11px] text-zinc-500">Lifetime requests served</div>
            </div>

            <div className="p-4 rounded-xl bg-zinc-900/50 border border-zinc-800 space-y-1">
              <div className="flex items-center justify-between text-xs text-zinc-400">
                <span>Success Rate (2xx)</span>
                <Zap className="w-4 h-4 text-amber-400" />
              </div>
              <div className="text-2xl font-bold font-mono text-emerald-400">
                {metrics?.total_requests
                  ? `${(((metrics.response_2xx || 0) / metrics.total_requests) * 100).toFixed(0)}%`
                  : "100%"}
              </div>
              <div className="text-[11px] text-zinc-500">
                {metrics?.response_2xx || 0} ok / {metrics?.response_5xx || 0} err
              </div>
            </div>
          </div>

          {/* Real-time Latency Chart */}
          <div className="p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800 space-y-4">
            <div className="flex items-center justify-between">
              <div className="text-xs font-semibold text-zinc-200">Real-Time Edge Latency (RTT)</div>
              <span className="text-[11px] font-mono text-zinc-500">Sampled every 3s</span>
            </div>

            <div className="h-56 w-full">
              {latencyHistory.length > 0 ? (
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={latencyHistory}>
                    <defs>
                      <linearGradient id="latencyGradient" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="#a855f7" stopOpacity={0.4} />
                        <stop offset="95%" stopColor="#a855f7" stopOpacity={0.0} />
                      </linearGradient>
                    </defs>
                    <XAxis
                      dataKey="time"
                      stroke="#52525b"
                      fontSize={10}
                      tickLine={false}
                    />
                    <YAxis
                      stroke="#52525b"
                      fontSize={10}
                      tickLine={false}
                      unit="ms"
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "#18181b",
                        borderColor: "#27272a",
                        borderRadius: "8px",
                        fontSize: "12px",
                      }}
                    />
                    <Area
                      type="monotone"
                      dataKey="rtt"
                      stroke="#a855f7"
                      strokeWidth={2}
                      fillOpacity={1}
                      fill="url(#latencyGradient)"
                    />
                  </AreaChart>
                </ResponsiveContainer>
              ) : (
                <div className="h-full flex items-center justify-center text-xs text-zinc-500 italic">
                  Collecting latency data points...
                </div>
              )}
            </div>
          </div>

          {/* Active Cloudflare Colos (Data Centers) */}
          <div className="p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800 space-y-3">
            <div className="text-xs font-semibold text-zinc-200">
              Connected Cloudflare Edge Data Centers (Colos)
            </div>
            <div className="flex flex-wrap gap-2">
              {metrics?.colos && metrics.colos.length > 0 ? (
                metrics.colos.map((colo) => (
                  <div
                    key={colo}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-zinc-950 border border-zinc-800 text-xs font-mono text-zinc-300"
                  >
                    <div className="w-2 h-2 rounded-full bg-emerald-400" />
                    <span className="font-bold">{colo}</span>
                    <span className="text-[10px] text-zinc-500">Cloudflare Edge</span>
                  </div>
                ))
              ) : (
                <div className="text-xs text-zinc-500 italic">
                  Connecting to closest Cloudflare anycast data centers...
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
};
