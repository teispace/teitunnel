# Teitunnel Security Policy & Credential Handling

Security is a primary design tenet of Teitunnel. Because Teitunnel interacts with Cloudflare API tokens, DNS records, and network tunnels that route incoming internet traffic to local ports, strict security controls are implemented at every layer.

---

## 1. Zero Plaintext Token Storage

- **OS-Level Keyring**:
  Cloudflare API Tokens are **never written to disk in plain text** (`.env`, `.json`, or config files).
  Instead, tokens are stored in the platform's native secure credential store via the Rust `keyring` crate:
  - **macOS**: Apple Keychain Services (encrypted with the user's login keychain)
  - **Windows**: Windows Credential Manager
  - **Linux**: Secret Service API via `libsecret` (GNOME Keyring / KWallet)

- **Memory Sanitization**:
  In-memory representations of tokens are kept scoped and dropped as soon as HTTP requests complete.

---

## 2. Ingress & Reverse Proxy Security

- **Strict Catch-All Enforced**:
  Every generated ingress configuration automatically includes a catch-all `http_status:404` rule at the bottom of the routing table. Any request not matching an explicitly authorized hostname and path is rejected immediately.

- **Localhost Boundary**:
  Default rules only route traffic to loopback interfaces (`127.0.0.1`, `localhost`, or Unix domain sockets). Binding to arbitrary external IP addresses requires explicit configuration.

- **TLS / SSL Verification**:
  By default, Teitunnel verifies upstream origin certificates. Disabling TLS verification (`noTLSVerify`) requires explicit user opt-in and is prominently flagged with a security warning badge.

---

## 3. DNS Safety & Dangling Record Prevention

- **Subdomain Takeover Prevention**:
  Orphaned DNS records pointing to defunct tunnel endpoints (`<uuid>.cfargotunnel.com`) can expose domains to takeover. Teitunnel implements:
  - **Cascade Deletion**: Prompts to remove corresponding CNAME records when a tunnel or route is deleted.
  - **DNS Hygiene Scanner**: Actively audits Cloudflare zones for dead tunnel CNAME pointers and provides 1-click batch remediation.

---

## 4. Binary Execution Integrity

- **Managed Binary Validation**:
  When downloading managed `cloudflared` binaries, Teitunnel downloads strictly from official Cloudflare GitHub releases (`https://github.com/cloudflare/cloudflared/releases`) over TLS with hash verification.
- **Controlled Process Spawn**:
  All child processes are launched with explicitly enumerated arguments. Shell string concatenation (`sh -c`) is strictly forbidden to prevent shell injection vulnerabilities.

---

## 5. Reporting Security Vulnerabilities

If you discover a security vulnerability within Teitunnel, please do NOT open a public GitHub issue.
Instead, report it responsibly to:
- **Email**: `security@teispace.com`
- Include detailed reproduction steps and affected versions.
We will acknowledge receipt within 48 hours and coordinate a coordinated disclosure timeline.
