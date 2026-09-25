/**
 * The few WebExtension APIs the popup uses (Chrome's `chrome.*`, which Firefox also
 * offers), declared here so the extension needs no type packages.
 */
interface ExtensionPort {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: (port: ExtensionPort) => void): void };
}

interface ExtensionRuntime {
  connectNative(application: string): ExtensionPort;
  lastError?: { message?: string } | undefined;
}

interface ExtensionTab {
  url?: string;
}

interface ExtensionTabs {
  query(filter: { active: boolean; currentWindow: boolean }): Promise<ExtensionTab[]>;
  create(properties: { url: string }): Promise<unknown>;
}

interface ExtensionApi {
  runtime: ExtensionRuntime;
  tabs: ExtensionTabs;
}

declare const chrome: ExtensionApi | undefined;
declare const browser: ExtensionApi | undefined;
