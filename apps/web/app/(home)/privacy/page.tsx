import type { Metadata } from "next";
import { ProsePage } from "@/components/prose-page";

export const metadata: Metadata = {
  title: "Privacy",
  description: "What Teitunnel sends where: nothing about you, and no telemetry.",
};

export default function Privacy() {
  return (
    <ProsePage title="Privacy" updated="September 23, 2026">
      <p>
        <strong>
          This program will not transfer any information to other networked systems unless
          specifically requested by the user or the person installing or operating it.
        </strong>{" "}
        Teitunnel has no telemetry, analytics or crash reporting, and no account with us.
      </p>
      <h2>What the app talks to</h2>
      <ul>
        <li>
          <strong>Cloudflare</strong>, with the credentials you connect, to do what you ask: read
          your domains and tunnels, and make the changes you review and apply.
        </li>
        <li>
          <strong>GitHub</strong>, to download and verify cloudflared when you ask, and to check for
          Teitunnel updates (one static file, once a day; you can turn it off in Settings). These
          requests carry nothing about you beyond what any web request does, such as your IP
          address.
        </li>
        <li>
          <strong>Your own routes</strong>, when it checks that a URL you published works.
        </li>
      </ul>
      <h2>What stays on your computer</h2>
      <p>
        Your Cloudflare credentials live only in your system's keychain. Settings, activity history
        and logs stay in Teitunnel's own folder. A diagnostics bundle is only made when you ask, is
        redacted, and you choose whether to share it.
      </p>
      <h2>This website</h2>
      <p>
        teitunnel.teispace.com is a static site hosted on GitHub Pages. It sets no cookies and runs
        no analytics. Downloads come from GitHub Releases. GitHub handles these requests under its
        own privacy statement.
      </p>
      <h2>Contact</h2>
      <p>
        Questions: <a href="mailto:info@teispace.com">info@teispace.com</a>. Security reports: see{" "}
        <a href="https://github.com/teispace/teitunnel/security/policy">SECURITY.md</a>.
      </p>
    </ProsePage>
  );
}
