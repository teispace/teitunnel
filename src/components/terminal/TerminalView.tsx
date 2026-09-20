import { useState, useRef, useEffect } from "react";
import {
  Terminal as TerminalIcon,
  Play,
  Trash2,
  Pause,
  Search,
  ChevronRight,
} from "lucide-react";

import { useLogStore } from "@/stores/log-store";
import { tauriApi } from "@/lib/tauri";

export const TerminalView: React.FC = () => {
  const {
    logs,
    filterLevel,
    searchQuery,
    isPaused,
    clearLogs,
    setFilterLevel,
    setSearchQuery,
    togglePause,
  } = useLogStore();

  const [customCommand, setCustomCommand] = useState("");
  const [isExecuting, setIsExecuting] = useState(false);
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isPaused && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs, isPaused]);

  const handleRunCommand = async (cmdStr: string) => {
    if (!cmdStr.trim() || isExecuting) return;
    setIsExecuting(true);
    const parts = cmdStr.trim().split(" ");
    const cmd = parts[0];
    const args = parts.slice(1);

    try {
      const res = await tauriApi.runTerminalCommand(cmd, args);
      setCommandHistory((prev) => [
        ...prev,
        `$ ${cmdStr}\n${res.stdout || ""}${res.stderr || ""}`,
      ]);
      setCustomCommand("");
    } catch (e: unknown) {
      setCommandHistory((prev) => [
        ...prev,
        `$ ${cmdStr}\nError: ${e instanceof Error ? e.message : String(e)}`,
      ]);
    } finally {
      setIsExecuting(false);
    }
  };

  const filteredLogs = logs.filter((log) => {
    if (filterLevel !== "ALL" && log.level !== filterLevel) return false;
    if (searchQuery && !log.line.toLowerCase().includes(searchQuery.toLowerCase())) {
      return false;
    }
    return true;
  });

  const quickCommands = [
    { label: "cloudflared --version", cmd: "cloudflared --version" },
    { label: "cloudflared tunnel list", cmd: "cloudflared tunnel list" },
    { label: "cloudflared --help", cmd: "cloudflared --help" },
  ];

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-zinc-950">
      {/* Top Toolbar */}
      <div className="h-12 border-b border-zinc-800 px-4 flex items-center justify-between gap-4 bg-zinc-900/60 select-none">
        <div className="flex items-center gap-2">
          <TerminalIcon className="w-4 h-4 text-zinc-400" />
          <span className="text-xs font-semibold text-zinc-200">Terminal & Logs</span>

          {/* Severity Filters */}
          <div className="flex items-center gap-1 ml-4 bg-zinc-950 border border-zinc-800 rounded-lg p-0.5 text-xs">
            {(["ALL", "INFO", "WARN", "ERR"] as const).map((lvl) => (
              <button
                key={lvl}
                onClick={() => setFilterLevel(lvl)}
                className={`px-2 py-0.5 rounded text-[11px] font-medium transition cursor-pointer ${
                  filterLevel === lvl
                    ? "bg-zinc-800 text-zinc-100 shadow-xs"
                    : "text-zinc-500 hover:text-zinc-300"
                }`}
              >
                {lvl}
              </button>
            ))}
          </div>
        </div>

        {/* Search & Actions */}
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-zinc-500 absolute left-2.5 top-2" />
            <input
              type="text"
              placeholder="Search logs..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-44 bg-zinc-950 border border-zinc-800 rounded-lg pl-8 pr-2.5 py-1 text-xs text-zinc-200 focus:outline-none focus:border-zinc-700"
            />
          </div>

          <button
            onClick={togglePause}
            className={`p-1.5 rounded-lg border transition cursor-pointer text-xs ${
              isPaused
                ? "bg-amber-950/50 border-amber-800 text-amber-300"
                : "bg-zinc-800/80 border-zinc-700 text-zinc-300 hover:bg-zinc-700"
            }`}
            title={isPaused ? "Resume log stream" : "Pause log stream"}
          >
            {isPaused ? <Play className="w-3.5 h-3.5 fill-current" /> : <Pause className="w-3.5 h-3.5" />}
          </button>

          <button
            onClick={clearLogs}
            className="p-1.5 rounded-lg bg-zinc-800/80 hover:bg-zinc-700 border border-zinc-700 text-zinc-300 transition cursor-pointer"
            title="Clear all logs"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Terminal Output Area */}
      <div
        ref={scrollRef}
        className="flex-1 p-4 font-mono text-xs overflow-y-auto space-y-1 select-text bg-zinc-950 text-zinc-300"
      >
        {/* Command executions */}
        {commandHistory.map((hist, i) => (
          <div key={i} className="whitespace-pre-wrap text-blue-300 border-b border-zinc-900 pb-2 mb-2">
            {hist}
          </div>
        ))}

        {/* Live streaming logs */}
        {filteredLogs.map((log, index) => {
          let badgeColor = "text-zinc-500";
          if (log.level === "ERR") badgeColor = "text-rose-400 font-bold";
          if (log.level === "WARN") badgeColor = "text-amber-400";
          if (log.level === "INFO") badgeColor = "text-emerald-400";

          return (
            <div key={index} className="flex items-start gap-2 leading-relaxed">
              <span className="text-[10px] text-zinc-600 shrink-0 select-none">
                {log.timestamp ? new Date(log.timestamp).toLocaleTimeString() : ""}
              </span>
              <span className={`text-[10px] uppercase font-bold shrink-0 ${badgeColor}`}>
                [{log.level}]
              </span>
              <span className="text-[11px] text-zinc-500 shrink-0 font-sans">
                {log.tunnel_id}:
              </span>
              <span className="break-all whitespace-pre-wrap">{log.line}</span>
            </div>
          );
        })}

        {filteredLogs.length === 0 && commandHistory.length === 0 && (
          <div className="h-full flex flex-col items-center justify-center text-zinc-600 space-y-2 select-none">
            <TerminalIcon className="w-8 h-8 text-zinc-800" />
            <p>Live stream waiting for active tunnel logs or commands...</p>
          </div>
        )}
      </div>

      {/* Quick Run & Command Input */}
      <div className="p-3 border-t border-zinc-800 bg-zinc-900/60 space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-zinc-500 font-medium">Quick commands:</span>
          {quickCommands.map((q) => (
            <button
              key={q.cmd}
              onClick={() => handleRunCommand(q.cmd)}
              disabled={isExecuting}
              className="text-[11px] font-mono px-2 py-0.5 rounded bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-zinc-200 hover:border-zinc-700 transition cursor-pointer"
            >
              {q.label}
            </button>
          ))}
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            handleRunCommand(customCommand);
          }}
          className="flex items-center gap-2"
        >
          <div className="flex-1 flex items-center gap-2 bg-zinc-950 border border-zinc-800 rounded-lg px-3 py-1.5 focus-within:border-blue-500">
            <ChevronRight className="w-4 h-4 text-blue-400 shrink-0" />
            <input
              type="text"
              value={customCommand}
              onChange={(e) => setCustomCommand(e.target.value)}
              placeholder="Run any cloudflared command (e.g. cloudflared tunnel list)..."
              className="w-full bg-transparent text-xs font-mono text-zinc-100 placeholder-zinc-600 focus:outline-none"
            />
          </div>

          <button
            type="submit"
            disabled={isExecuting || !customCommand.trim()}
            className="px-4 py-1.5 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white text-xs font-semibold rounded-lg shadow-sm transition cursor-pointer"
          >
            {isExecuting ? "Running..." : "Execute"}
          </button>
        </form>
      </div>
    </div>
  );
};
