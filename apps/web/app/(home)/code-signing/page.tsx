import type { Metadata } from "next";
import { ProsePage } from "@/components/prose-page";

export const metadata: Metadata = {
  title: "Code signing policy",
  description: "How Teitunnel's downloads and updates are built, signed and verified.",
};

export default function CodeSigning() {
  return (
    <ProsePage title="Code signing policy" updated="September 23, 2026">
      <p>
        Every Teitunnel download is built from this repository's source by GitHub Actions, from a
        tagged commit, and nothing is built or signed on a personal computer. Releases are made by
        the release workflow (<code>.github/workflows/release.yml</code>) after a maintainer
        approves the release pull request.
      </p>
      <h2>How each download is signed</h2>
      <ul>
        <li>
          <strong>macOS:</strong> signed with the Developer ID of Teispace, with the hardened
          runtime, and notarized by Apple.
        </li>
        <li>
          <strong>Windows:</strong> not yet code-signed. We're applying for free code signing for
          open source projects from SignPath Foundation; once it's in place, this page will say so
          and Windows installers will be signed with its certificate.
        </li>
        <li>
          <strong>Linux:</strong> the packages aren't signed by a distribution; verify them with the
          checksums and build provenance below.
        </li>
        <li>
          <strong>Updates:</strong> every update is signed with Teispace's update key, and the app
          refuses any update whose signature doesn't match the key built into it.
        </li>
      </ul>
      <h2>Verifying a download</h2>
      <p>
        Each release lists every file's SHA-256 in <code>SHA256SUMS.txt</code> and carries GitHub
        build provenance, which ties the file to the workflow run and commit that built it:{" "}
        <code>gh attestation verify &lt;file&gt; --repo teispace/teitunnel</code>.
      </p>
      <h2>Team and roles</h2>
      <ul>
        <li>
          <strong>Committers and reviewers:</strong> the maintainers in the{" "}
          <a href="https://github.com/teispace">Teispace organization</a>. Changes from other
          contributors are reviewed by a maintainer before they're merged.
        </li>
        <li>
          <strong>Approvers:</strong> Teispace's owners, who approve each release.
        </li>
      </ul>
      <p>
        All team members use multi-factor authentication for GitHub. Only software built from this
        repository is signed; the cloudflared binary Teitunnel downloads is Cloudflare's own, and
        Teitunnel checks Cloudflare's signature and checksums before using it.
      </p>
      <h2>Privacy</h2>
      <p>
        This program will not transfer any information to other networked systems unless
        specifically requested by the user or the person installing or operating it. See{" "}
        <a href="/privacy/">Privacy</a>.
      </p>
    </ProsePage>
  );
}
