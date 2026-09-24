import {
  Activity,
  Camera,
  ChartNoAxesColumn,
  FolderGit2,
  Globe,
  LayoutGrid,
  type LucideIcon,
  Network,
  ScanSearch,
  Share,
  Stethoscope,
  Waypoints,
} from "lucide-react";
import type { MessageKey } from "@/lib/i18n";

export interface NavItem {
  readonly to:
    | "/"
    | "/routes"
    | "/quick-share"
    | "/inspector"
    | "/snapshots"
    | "/projects"
    | "/domains"
    | "/tunnels"
    | "/activity"
    | "/doctor"
    | "/analytics";
  readonly label: MessageKey;
  readonly icon: LucideIcon;
}

export interface NavSection {
  readonly title: MessageKey | null;
  readonly items: readonly NavItem[];
}

/** Sidebar information architecture. ⌘1–⌘9 follow this order (Inspector and Projects have none). */
export const navigation: readonly NavSection[] = [
  {
    title: null,
    items: [
      { to: "/", label: "nav.overview", icon: LayoutGrid },
      { to: "/routes", label: "nav.routes", icon: Waypoints },
      { to: "/quick-share", label: "nav.quickShare", icon: Share },
      { to: "/inspector", label: "nav.inspector", icon: ScanSearch },
      { to: "/snapshots", label: "nav.snapshots", icon: Camera },
      { to: "/projects", label: "nav.projects", icon: FolderGit2 },
    ],
  },
  {
    title: "nav.cloudflare",
    items: [
      { to: "/domains", label: "nav.domains", icon: Globe },
      { to: "/tunnels", label: "nav.tunnels", icon: Network },
    ],
  },
  {
    title: "nav.health",
    items: [
      { to: "/activity", label: "nav.activity", icon: Activity },
      { to: "/doctor", label: "nav.doctor", icon: Stethoscope },
      { to: "/analytics", label: "nav.analytics", icon: ChartNoAxesColumn },
    ],
  },
];

export const navItems: readonly NavItem[] = navigation.flatMap((section) => section.items);
