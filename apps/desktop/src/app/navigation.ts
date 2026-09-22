import {
  Activity,
  Globe,
  LayoutGrid,
  type LucideIcon,
  Network,
  Share,
  Stethoscope,
  Waypoints,
} from "lucide-react";

export interface NavItem {
  readonly to: "/" | "/routes" | "/quick-share" | "/domains" | "/tunnels" | "/activity" | "/doctor";
  readonly label: string;
  readonly icon: LucideIcon;
}

export interface NavSection {
  readonly title: string | null;
  readonly items: readonly NavItem[];
}

/** Sidebar information architecture. ⌘1–⌘7 follow this order. */
export const navigation: readonly NavSection[] = [
  {
    title: null,
    items: [
      { to: "/", label: "Overview", icon: LayoutGrid },
      { to: "/routes", label: "Routes", icon: Waypoints },
      { to: "/quick-share", label: "Quick Share", icon: Share },
    ],
  },
  {
    title: "Cloudflare",
    items: [
      { to: "/domains", label: "Domains", icon: Globe },
      { to: "/tunnels", label: "Tunnels", icon: Network },
    ],
  },
  {
    title: "Health",
    items: [
      { to: "/activity", label: "Activity", icon: Activity },
      { to: "/doctor", label: "Doctor", icon: Stethoscope },
    ],
  },
];

export const navItems: readonly NavItem[] = navigation.flatMap((section) => section.items);
