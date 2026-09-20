import {
  Globe,
  Radio,
  Cloud,
  Download,
  CheckCircle2,
} from "lucide-react";
import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";

interface TitleBarProps {
  onOpenSettings: () => void;
}

export const TitleBar: React.FC<TitleBarProps> = ({ onOpenSettings }) => {
  const { accounts, activeAccountId, selectAccount, zones, activeZoneId, selectZone, token } =
    useAuthStore();
  const { status, isDownloading, downloadManagedBinary } = useBinaryStore();


  return (
    <header className="h-11 bg-zinc-950/80 border-b border-zinc-800/60 backdrop-blur-xl flex items-center justify-between px-4 select-none z-50 drag-region">
      {/* Left: macOS traffic lights offset & Title */}
      <div className="flex items-center gap-2 pl-18">
        <div className="flex items-center gap-2">
          <div className="w-5 h-5 rounded-md bg-blue-600/20 border border-blue-500/40 flex items-center justify-center text-blue-400">
            <Radio className="w-3 h-3 animate-pulse" />
          </div>
          <span className="font-semibold text-xs text-zinc-100 tracking-wide flex items-center gap-1.5">
            Teitunnel
            <span className="text-[10px] text-zinc-400 font-normal px-1.5 py-0.5 rounded bg-zinc-800/80 border border-zinc-700/50">
              v0.1.0
            </span>
          </span>
        </div>
      </div>

      {/* Middle: Cloudflare Account & Zone Selector (if authenticated) */}
      <div className="flex items-center gap-2 no-drag">
        {token && accounts.length > 0 ? (
          <div className="flex items-center gap-1.5 bg-zinc-900/90 border border-zinc-800 px-2 py-1 rounded-md text-xs">
            <Cloud className="w-3.5 h-3.5 text-orange-400" />
            <select
              value={activeAccountId || ""}
              onChange={(e) => selectAccount(e.target.value)}
              className="bg-transparent text-zinc-200 focus:outline-none text-xs cursor-pointer font-medium"
            >
              {accounts.map((acc) => (
                <option key={acc.id} value={acc.id} className="bg-zinc-900 text-zinc-200">
                  {acc.name}
                </option>
              ))}
            </select>

            {zones.length > 0 && (
              <>
                <span className="text-zinc-600">/</span>
                <Globe className="w-3.5 h-3.5 text-blue-400" />
                <select
                  value={activeZoneId || ""}
                  onChange={(e) => selectZone(e.target.value)}
                  className="bg-transparent text-zinc-200 focus:outline-none text-xs cursor-pointer font-medium"
                >
                  {zones.map((zone) => (
                    <option key={zone.id} value={zone.id} className="bg-zinc-900 text-zinc-200">
                      {zone.name}
                    </option>
                  ))}
                </select>
              </>
            )}
          </div>
        ) : (
          <button
            onClick={onOpenSettings}
            className="flex items-center gap-1.5 text-xs text-zinc-400 hover:text-zinc-200 bg-zinc-900/60 hover:bg-zinc-800/80 border border-zinc-800/80 px-2.5 py-1 rounded-md transition cursor-pointer"
          >
            <Cloud className="w-3.5 h-3.5 text-zinc-400" />
            <span>Connect Cloudflare Account</span>
          </button>
        )}
      </div>

      {/* Right: Binary Status Indicator */}
      <div className="flex items-center gap-2 no-drag">
        {status?.is_installed ? (
          <div
            onClick={onOpenSettings}
            title={`cloudflared installed at ${status.path || ""}`}
            className="flex items-center gap-1.5 text-xs text-emerald-400 bg-emerald-950/40 border border-emerald-800/40 px-2 py-0.5 rounded-full cursor-pointer hover:bg-emerald-900/40 transition"
          >
            <CheckCircle2 className="w-3 h-3" />
            <span className="text-[11px] font-medium">cloudflared ready</span>
          </div>
        ) : (
          <button
            onClick={downloadManagedBinary}
            disabled={isDownloading}
            className="flex items-center gap-1.5 text-xs text-amber-400 bg-amber-950/40 border border-amber-800/40 px-2 py-0.5 rounded-full cursor-pointer hover:bg-amber-900/40 transition disabled:opacity-60"
          >
            <Download className="w-3 h-3 animate-bounce" />
            <span className="text-[11px] font-medium">
              {isDownloading ? "Downloading..." : "Download cloudflared"}
            </span>
          </button>
        )}
      </div>
    </header>
  );
};
