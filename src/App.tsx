import { useEffect, useState } from "react";
import { Toaster } from "sonner";

import { TitleBar } from "@/components/layout/TitleBar";
import { Sidebar, type NavTab } from "@/components/layout/Sidebar";
import { StatusBar } from "@/components/layout/StatusBar";
import { QuickTunnelView } from "@/components/quick-tunnel/QuickTunnelView";
import { TunnelsView } from "@/components/tunnels/TunnelsView";
import { IngressView } from "@/components/ingress/IngressView";
import { DnsHygieneView } from "@/components/dns/DnsHygieneView";
import { MetricsView } from "@/components/metrics/MetricsView";
import { TerminalView } from "@/components/terminal/TerminalView";
import { SettingsView } from "@/components/settings/SettingsView";
import { useAuthStore } from "@/stores/auth-store";
import { useBinaryStore } from "@/stores/binary-store";
import { useQuickTunnelStore } from "@/stores/quick-tunnel-store";
import { useTunnelStore } from "@/stores/tunnel-store";
import { useLogStore } from "@/stores/log-store";
import { tauriApi, type CloudflareTunnel } from "@/lib/tauri";

export function App() {
  const [currentTab, setCurrentTab] = useState<NavTab>("quick-tunnel");
  const [contextTunnel, setContextTunnel] = useState<CloudflareTunnel | null>(null);

  const {
    initAuth,
    setBrowserLoginUrl,
    setBrowserLoginSuccess,
    setBrowserLoginFailed,
  } = useAuthStore();
  const { checkStatus } = useBinaryStore();
  const { fetchState, setPublicUrl, setStopped } = useQuickTunnelStore();
  const { updateTunnelProcessState, initTunnels } = useTunnelStore();
  const { addLog } = useLogStore();

  useEffect(() => {
    // 1. Initialize stores
    initAuth();
    initTunnels();
    checkStatus();
    fetchState();

    // 2. Set up global event listeners from Rust backend
    let unlistenLog: (() => void) | undefined;
    let unlistenReady: (() => void) | undefined;
    let unlistenStopped: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;
    let unlistenLoginUrl: (() => void) | undefined;
    let unlistenLoginSuccess: (() => void) | undefined;
    let unlistenLoginFailed: (() => void) | undefined;

    tauriApi.onLog((log) => {
      addLog(log);
    }).then((un) => {
      unlistenLog = un;
    });

    tauriApi.onQuickTunnelReady((url) => {
      setPublicUrl(url);
    }).then((un) => {
      unlistenReady = un;
    });

    tauriApi.onQuickTunnelStopped(() => {
      setStopped();
    }).then((un) => {
      unlistenStopped = un;
    });

    tauriApi.onTunnelStatusChanged(([tid, status]) => {
      updateTunnelProcessState(tid, status as "running" | "stopped");
    }).then((un) => {
      unlistenStatus = un;
    });

    tauriApi.onBrowserLoginUrl((url) => {
      setBrowserLoginUrl(url);
    }).then((un) => {
      unlistenLoginUrl = un;
    });

    tauriApi.onBrowserLoginSuccess(() => {
      setBrowserLoginSuccess();
    }).then((un) => {
      unlistenLoginSuccess = un;
    });

    tauriApi.onBrowserLoginFailed((err) => {
      setBrowserLoginFailed(err);
    }).then((un) => {
      unlistenLoginFailed = un;
    });

    return () => {
      if (unlistenLog) unlistenLog();
      if (unlistenReady) unlistenReady();
      if (unlistenStopped) unlistenStopped();
      if (unlistenStatus) unlistenStatus();
      if (unlistenLoginUrl) unlistenLoginUrl();
      if (unlistenLoginSuccess) unlistenLoginSuccess();
      if (unlistenLoginFailed) unlistenLoginFailed();
    };
  }, []);

  const handleNavigateIngress = (tunnel: CloudflareTunnel) => {
    setContextTunnel(tunnel);
    setCurrentTab("ingress");
  };

  const handleNavigateDns = (tunnel: CloudflareTunnel) => {
    setContextTunnel(tunnel);
    setCurrentTab("dns");
  };

  const handleNavigateTelemetry = (tunnel: CloudflareTunnel) => {
    setContextTunnel(tunnel);
    setCurrentTab("telemetry");
  };

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-zinc-950 text-zinc-100 select-none">
      {/* Native Title Bar */}
      <TitleBar
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        onOpenSettings={() => setCurrentTab("settings")}
      />

      {/* Main Content Area */}
      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <Sidebar currentTab={currentTab} onTabChange={setCurrentTab} />

        {/* Dynamic View */}
        <main className="flex-1 flex flex-col overflow-hidden bg-zinc-925">
          {currentTab === "quick-tunnel" && <QuickTunnelView />}
          {currentTab === "tunnels" && (
            <TunnelsView
              onNavigateIngress={handleNavigateIngress}
              onNavigateDns={handleNavigateDns}
              onNavigateTelemetry={handleNavigateTelemetry}
            />
          )}
          {currentTab === "ingress" && <IngressView initialTunnel={contextTunnel} />}
          {currentTab === "dns" && <DnsHygieneView />}
          {currentTab === "telemetry" && <MetricsView initialTunnel={contextTunnel} />}
          {currentTab === "terminal" && <TerminalView />}
          {currentTab === "settings" && <SettingsView />}
        </main>
      </div>

      {/* Native Bottom Status Bar */}
      <StatusBar currentTab={currentTab} onTabChange={setCurrentTab} />

      {/* Modern Toast Feedback */}
      <Toaster
        theme="dark"
        position="bottom-right"
        toastOptions={{
          className: "bg-zinc-900/95 border border-zinc-800 text-zinc-100 shadow-2xl backdrop-blur-md text-xs font-mono",
        }}
        richColors
      />
    </div>
  );
}

export default App;
