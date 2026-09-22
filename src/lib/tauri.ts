import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Models
export interface BinaryStatus {
  is_installed: boolean;
  path: string | null;
  version: string | null;
  is_managed: boolean;
  architecture: string;
  os: string;
}


export interface CertStatus {
  has_cert: boolean;
  cert_path: string | null;
  zone_id?: string | null;
  account_id?: string | null;
  api_token?: string | null;
}

export interface DirectTunnel {
  id: string;
  name: string;
  token: string;
  createdAt: string;
}

export interface DownloadProgress {
  bytes_downloaded: number;
  total_bytes: number | null;
  percentage: number | null;
  status: string;
}

export interface CloudflareAccount {
  id: string;
  name: string;
}

export interface CloudflareZone {
  id: string;
  name: string;
  status: string;
  paused: boolean;
}

export interface TunnelConnection {
  id: string;
  features?: string[];
  version?: string;
  arch?: string;
  colo_name?: string;
  is_pending_reconnect?: boolean;
  opened_at?: string;
}

export interface CloudflareTunnel {
  id: string;
  name: string;
  status?: string;
  created_at?: string;
  deleted_at?: string;
  connections: TunnelConnection[];
  remote_config: boolean;
}

export interface TunnelProcessState {
  tunnel_id: string;
  pid?: number;
  is_running: boolean;
  started_at?: string;
  metrics_port?: number;
  mode: string;
}

export interface QuickTunnelState {
  is_running: boolean;
  pid?: number;
  local_port: number;
  public_url?: string;
  started_at?: string;
  logs: string[];
}

export interface OriginRequestConfig {
  connect_timeout?: string;
  tls_timeout?: string;
  tcp_keep_alive?: string;
  no_tls_verify?: boolean;
  origin_server_name?: string;
  ca_pool?: string;
  http_host_header?: string;
  disable_chunked_encoding?: boolean;
}

export interface IngressRule {
  hostname?: string;
  path?: string;
  service: string;
  origin_request?: OriginRequestConfig;
}

export interface TunnelConfiguration {
  ingress: IngressRule[];
}

export interface DnsRecord {
  id: string;
  zone_id: string;
  zone_name?: string;
  name: string;
  type: string;
  content: string;
  proxied: boolean;
  ttl: number;
  comment?: string;
  created_on?: string;
  modified_on?: string;
}

export interface OrphanedDnsRecord {
  record: DnsRecord;
  target_tunnel_uuid: string;
  is_orphaned: boolean;
  reason: string;
}

export interface DnsHygieneReport {
  total_cnames_scanned: number;
  tunnel_cnames_count: number;
  orphaned_records: OrphanedDnsRecord[];
}

export interface TunnelMetrics {
  tunnel_id: string;
  timestamp: string;
  active_connections: number;
  colos: string[];
  avg_rtt_ms: number;
  total_requests: number;
  response_2xx: number;
  response_4xx: number;
  response_5xx: number;
  bytes_in: number;
  bytes_out: number;
}

export interface TunnelLogEvent {
  tunnel_id: string;
  line: string;
  level: string;
  timestamp: string;
}

export interface CommandResult {
  stdout: string;
  stderr: string;
  exit_code: number | null;
  success: boolean;
}

// IPC Wrapper
export const tauriApi = {
  // Binary
  checkBinaryStatus: () => invoke<BinaryStatus>("check_binary_status"),
  downloadManagedBinary: () => invoke<BinaryStatus>("download_managed_binary"),

  // Auth & Accounts
  verifyAndSaveToken: (token: string) => invoke<boolean>("verify_and_save_token", { token }),
  getSavedToken: () => invoke<string | null>("get_saved_token"),
  deleteSavedToken: () => invoke<void>("delete_saved_token"),
  listAccounts: (token?: string) => invoke<CloudflareAccount[]>("list_accounts", { token }),
  listZones: (accountId: string, token?: string) =>
    invoke<CloudflareZone[]>("list_zones", { accountId, token }),

  // Origin Cert & Browser Login (Zero API Token)
  checkCertStatus: () => invoke<CertStatus>("check_cert_status"),
  startBrowserLogin: () => invoke<void>("start_browser_login"),
  cancelBrowserLogin: () => invoke<void>("cancel_browser_login"),
  deleteCert: () => invoke<void>("delete_cert"),

  // Tunnels
  listTunnels: (accountId: string, token?: string) =>
    invoke<CloudflareTunnel[]>("list_tunnels", { accountId, token }),
  createTunnel: (accountId: string, name: string, token?: string) =>
    invoke<CloudflareTunnel>("create_tunnel", { accountId, name, token }),
  startTunnel: (accountId: string, tunnelId: string, token?: string) =>
    invoke<TunnelProcessState>("start_tunnel", { accountId, tunnelId, token }),
  stopTunnel: (tunnelId: string) => invoke<void>("stop_tunnel", { tunnelId }),
  deleteTunnel: (accountId: string, tunnelId: string, token?: string) =>
    invoke<void>("delete_tunnel", { accountId, tunnelId, token }),
  getActiveProcesses: () => invoke<TunnelProcessState[]>("get_active_processes"),

  // Direct Token & Named Tunnel (Easy Run)
  startTunnelByToken: (tunnelId: string, token: string) =>
    invoke<TunnelProcessState>("start_tunnel_by_token", { tunnelId, token }),
  startNamedTunnel: (tunnelName: string) =>
    invoke<TunnelProcessState>("start_named_tunnel", { tunnelName }),
  listCertTunnels: () => invoke<CloudflareTunnel[]>("list_cert_tunnels"),
  createCertTunnel: (name: string) => invoke<CloudflareTunnel>("create_cert_tunnel", { name }),
  deleteCertTunnel: (tunnelId: string) => invoke<void>("delete_cert_tunnel", { tunnelId }),

  // Quick Ephemeral Tunnel
  startQuickTunnel: (localPort: number) =>
    invoke<QuickTunnelState>("start_quick_tunnel", { localPort }),
  stopQuickTunnel: () => invoke<void>("stop_quick_tunnel"),
  getQuickTunnelState: () => invoke<QuickTunnelState | null>("get_quick_tunnel_state"),

  // Ingress
  getTunnelConfiguration: (accountId: string, tunnelId: string, token?: string) =>
    invoke<TunnelConfiguration>("get_tunnel_configuration", { accountId, tunnelId, token }),
  updateTunnelConfiguration: (
    accountId: string,
    tunnelId: string,
    rules: IngressRule[],
    token?: string
  ) =>
    invoke<TunnelConfiguration>("update_tunnel_configuration", {
      accountId,
      tunnelId,
      rules,
      token,
    }),

  // DNS & Hygiene
  listDnsRecords: (zoneId: string, recordType?: string, token?: string) =>
    invoke<DnsRecord[]>("list_dns_records", { zoneId, recordType, token }),
  createDnsCname: (zoneId: string, name: string, tunnelUuid: string, token?: string) =>
    invoke<DnsRecord>("create_dns_cname", { zoneId, name, tunnelUuid, token }),
  deleteDnsRecord: (zoneId: string, recordId: string, token?: string) =>
    invoke<void>("delete_dns_record", { zoneId, recordId, token }),
  scanDnsHygiene: (accountId: string, zoneId: string, token?: string) =>
    invoke<DnsHygieneReport>("scan_dns_hygiene", { accountId, zoneId, token }),

  // Telemetry
  getTunnelMetrics: (tunnelId: string, metricsPort: number) =>
    invoke<TunnelMetrics>("get_tunnel_metrics", { tunnelId, metricsPort }),

  // Terminal
  runTerminalCommand: (command: string, args: string[]) =>
    invoke<CommandResult>("run_terminal_command", { command, args }),

  // Events
  onLog: (callback: (log: TunnelLogEvent) => void): Promise<UnlistenFn> =>
    listen<TunnelLogEvent>("tunnel-log", (e) => callback(e.payload)),
  onQuickTunnelReady: (callback: (url: string) => void): Promise<UnlistenFn> =>
    listen<string>("quick-tunnel-ready", (e) => callback(e.payload)),
  onQuickTunnelStopped: (callback: () => void): Promise<UnlistenFn> =>
    listen<void>("quick-tunnel-stopped", () => callback()),
  onTunnelStatusChanged: (
    callback: (data: [string, string]) => void
  ): Promise<UnlistenFn> =>
    listen<[string, string]>("tunnel-status-changed", (e) => callback(e.payload)),
  onBrowserLoginUrl: (callback: (url: string) => void): Promise<UnlistenFn> =>
    listen<string>("browser-login-url", (e) => callback(e.payload)),
  onBrowserLoginSuccess: (callback: () => void): Promise<UnlistenFn> =>
    listen<void>("browser-login-success", () => callback()),
  onBrowserLoginFailed: (callback: (err: string) => void): Promise<UnlistenFn> =>
    listen<string>("browser-login-failed", (e) => callback(e.payload)),
};
