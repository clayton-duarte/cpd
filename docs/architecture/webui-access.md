# WebUI access

The owner chose **loopback by default, authenticated LAN as an opt-in** on
2026-11-14. This applies to production (3333) and preview (33333), independently
on every backend. It supersedes the earlier unauthenticated wildcard defaults.

For LAN access, configure the backend service environment:

```sh
export CPD_WEBUI_HOST=0.0.0.0
export CPD_WEBUI_ACCESS_TOKEN='<a separate random token of at least 32 characters>'
```

Generate each token with a cryptographic generator, such as `openssl rand -hex
32`, and keep it in that machine's private service configuration. A LAN binding
without a sufficiently long token refuses startup. Authenticate in the browser
as `cpd`, using that backend's token as the password. HTTP Basic uses the browser's
standard authentication cache, also used by same-origin WebSocket upgrades.
Open each backend's UI at its own origin. Chromium partitions its HTTP
authentication cache: authenticating backend B in a tab does **not** authorize a
WebSocket from UI A to B. This was reproduced with a fresh Chromium profile and
the production access policy; same-origin authentication succeeded and the
cross-origin socket was refused. Do not depend on cached Basic credentials for
cross-backend connections.

The current browser entry point (`web/src/main.tsx`) opens a same-origin socket;
it does not implement the requested three-backend dropdown. That product goal
remains three independent instances, with no shared database or backend sync.
A future selector must either keep HTTP and WebSocket requests same-origin
through an authenticated gateway with separate upstream credentials, or provide
an explicit browser-compatible authentication flow for each backend. Merely
adding an Origin allowlist is insufficient. This remediation does not claim to
implement that selector.

`CPD_WEBUI_ALLOWED_ORIGINS` accepts a comma-separated list of exact trusted UI
origins (scheme, host and port). It permits an origin; it does not supply a
credential. Same-origin connections need no extra entry. Wildcards and paths are
refused. Preserve the Host header through a reverse proxy. A private network
address alone does not make a browser origin trusted. Each backend owns its own
store, processes and credential.

Use HTTPS through a trusted reverse proxy for LAN access: Basic credentials are
encoded, not encrypted. The proxy must forward Authorization and WebSocket
upgrades; restrict direct backend access to trusted networks. HTTPS Origins on
the default HTTPS port or the two CPD ports are accepted for the existing
local/LAN host allowlist. Origin validation is not authentication. Static routes,
CPD sockets and preview HMR upgrades require the credential. This credential is
separate from the per-process durable-host WebUI token and never enters URLs or
logs. The default local binding needs no client credential.

The [audit ledger](../research/2026-11-14-review-audit-remediation.md) records the
owner decision, OWASP guidance, implementation and validation. No live host
configuration or deployment is performed by this change.
