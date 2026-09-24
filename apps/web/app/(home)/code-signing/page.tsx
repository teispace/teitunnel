import type { Metadata } from "next";
import { ProsePage } from "@/components/prose-page";
import { ogImage, pageMetadata } from "@/lib/seo";

export const metadata: Metadata = pageMetadata({
  title: "Code signing policy",
  description: "How Teitunnel's downloads and updates are built, signed and verified.",
  path: "/code-signing/",
  image: ogImage(["code-signing"]),
});

export default function CodeSigning() {
  return (
    <ProsePage title="Code signing policy" updated="September 24, 2026">
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
          <strong>Windows:</strong> free code signing provided by{" "}
          <a href="https://signpath.io">SignPath.io</a>, certificate by{" "}
          <a href="https://signpath.org">SignPath Foundation</a>. Our application is being reviewed;
          until it's approved, Windows installers are unsigned, and this page will say when that
          changes.
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
          <strong>Committers and reviewers:</strong>{" "}
          <a href="https://github.com/orgs/teispace/people">members of the Teispace organization</a>
          . Every change goes through a pull request on <code>main</code>, and changes from anyone
          outside the organization are reviewed by a member before they're merged.
        </li>
        <li>
          <strong>Approvers:</strong>{" "}
          <a href="https://github.com/orgs/teispace/people?query=role%3Aowner">
            Teispace's organization owners
          </a>
          , who approve each release and each signing request.
        </li>
      </ul>
      <p>
        Everyone on the team uses multi-factor authentication for GitHub and for SignPath. Only
        software built from this repository is signed: release builds run on GitHub Actions from a
        tagged commit, and nothing is signed on a personal computer. The desktop app downloads
        Cloudflare's own cloudflared when you ask and checks Cloudflare's signature and checksums
        before using it; the Docker image includes it unmodified. We never sign cloudflared.
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
