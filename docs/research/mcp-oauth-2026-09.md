# OAuth for MCP servers (checked 2026-09-26)

Facts behind D-132 (`crates/core/src/mcp_auth.rs`, Lens's `OAuthProvider`).

## The MCP authorization spec, revision 2026-07-28

Source: [Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization),
[Authorization Server Discovery](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery),
[Client Registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration),
[Security Considerations](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations).

- The MCP server is an OAuth 2.1 resource server and **must** publish Protected Resource
  Metadata (RFC 9728) with at least one `authorization_servers` entry. It either sends
  `WWW-Authenticate: Bearer resource_metadata="…"` on 401, or serves the metadata at
  `/.well-known/oauth-protected-resource/<mcp path>` or at the root. Clients try the header,
  then the path, then the root.
- Authorization servers must offer RFC 8414 metadata or OpenID Connect discovery; clients try,
  for an issuer without a path, `/.well-known/oauth-authorization-server` then
  `/.well-known/openid-configuration`, and must check `issuer` equals the issuer they used.
- Clients must see `code_challenge_methods_supported` (PKCE, S256) or refuse to continue.
- Client registration, in the client's order of preference: pre-registered, **Client ID
  Metadata Documents** (advertised with `client_id_metadata_document_supported: true`),
  **Dynamic Client Registration** (`registration_endpoint`; RFC 7591, now deprecated but kept
  for compatibility). CIMD: `client_id` is an HTTPS URL with a path; the document must hold
  `client_id` (equal to the URL), `client_name`, `redirect_uris`; the server must check the
  `client_id` match and the redirect URI, should mind SSRF, must show the redirect URI host,
  should warn about localhost-only redirects.
- Authorization servers should send `iss` in authorization responses (RFC 9207) and then must
  advertise `authorization_response_iss_parameter_supported: true`; clients compare it with the
  issuer they recorded.
- Clients must send `resource` (RFC 8707, the MCP server's canonical URI) in authorization and
  token requests; servers must accept only tokens issued for them, and must not pass tokens
  through to anything upstream.
- Tokens only in the `Authorization: Bearer` header, never the query. Invalid or expired
  tokens get 401; insufficient scope 403 with `error="insufficient_scope"`.
- Authorization servers should issue short-lived access tokens and must rotate refresh tokens
  for public clients. All endpoints over HTTPS; redirect URIs must be HTTPS or localhost and are
  matched exactly.

## Clients

- **claude.ai** (custom connectors): callback `https://claude.ai/api/mcp/auth_callback`
  (possibly `https://claude.com/api/mcp/auth_callback` later); its connector dialog recommends
  CIMD with a document Anthropic hosts, and also supports DCR and pre-registered credentials.
  ([Anthropic help](https://support.anthropic.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers),
  [sunpeak, July 2026](https://sunpeak.ai/blogs/claude-connector-oauth-authentication/))
- **ChatGPT** (apps and connectors): supports CIMD, DCR and a predefined client; recommends CIMD
  for new work; redirect on chatgpt.com (e.g. `https://chatgpt.com/connector_platform_oauth_redirect`).
  ([OpenAI, Authentication](https://developers.openai.com/plugins/build/auth),
  [Zuplo compatibility notes](https://zuplo.com/learn/mcp/compatibility/clients/chatgpt-connectors))
- Claude Code, Cursor and VS Code discover OAuth from the 401 and register dynamically; they
  also keep working with a static `Authorization` header in their configuration.

## Not verified yet

- A real claude.ai and ChatGPT connection through a shared server (needs a live domain and the
  maintainer's accounts).
