# Progressive Web App (PWA) Shell

This document describes the PWA contract for the WebUI: offline caching, service-worker scope and headers, engine status banner, outbox queue, server deduplication, and HTTPS requirements.

**Status:** Web/CONTRACT.md §34 defines the wire types and state machines. Implementation is deferred to a later Job (pro-d591 for the full PWA feature, pro-d571 for HTTPS provisioning and LAN certificate setup).

## 1. How the PWA behaves

The WebUI is a Progressive Web App with the following features:

### 1.1 Service-worker registration

- **Browser:** The app registers a service worker at `/sw.js` (root scope) only in a **secure context** (HTTPS, `localhost`, `127.0.0.1`, or `::1`). See §2 for HTTPS provisioning on a LAN.
- **Electron:** When `window.cpdElectron` is defined, no service worker is registered. Electron runs cpd locally via IPC; offline caching is not needed.
- **Registrationscript:** `navigator.serviceWorker.register('/sw.js')` in `web/src/main.tsx` (conditional on not Electron); unimplemented until pro-d591.

### 1.2 Offline outbox

When the WebSocket connection is **closed** or **reconnecting**, user submissions are queued in a client-side IndexedDB store (outbox) instead of being sent immediately. When the connection reopens, the outbox is replayed in FIFO order by creation time.

**Outbox scope:** Composer submissions (`session.submit`), dialog responses (`dialog.respond`), and cockpit actions of types `inbox.*`, `pr.*`, `session.retier`, `plan.reject`, `context.retire`, and `engine.start`.

**Outbox entry fields:**
- `id`: clientSubmissionId, actionId, or dialogId (the server's dedup key)
- `kind`: `"submit" | "action" | "respond"` (message type)
- `payload`: the complete message payload
- `state`: `"queued" | "sending" | "failed"` (lifecycle state)
- `error`: WireError if `state === "failed"`
- `createdAt`: ISO timestamp

**Outbox replay:**
1. When the socket transitions to `open`, fetch all `queued` entries in `createdAt` order.
2. For each entry, send the message to the server and mark `state: "sending"`.
3. On server ack/result/resolved (success or 4xx error), delete the entry.
4. On 5xx error or network error, revert to `state: "queued"` for the next reconnect.
5. Failed entries stay in the store with `state: "failed"` and an error detail (display to user; clearing is a user action, TBD).

### 1.3 Engine status banner

A global banner displays the top-level connection and engine state, separate from any selected session. The banner is a full-width bar above the Shell's 5 zones (Header, Tree, Plan, Chat, Inbox, StatusBar).

**Banner kinds (see §3 for state machine):**
- **`reconnecting`:** WebSocket is closed/reconnecting. Display: `"Reconnecting…"`.
- **`server-unreachable`:** Socket is closed/reconnecting and was reachable before. Display: `"Server unreachable"` with timestamp.
- **`engine-down`:** Socket is open but `InboxModel.engineRunning === false`. Display: `"Engine down"` with timestamp.
- **`engine-restarting`:** Engine is down and `engineRestartReason === "install"`. Display: `"Engine restarting from install…"`.
- **none:** No banner displayed.

**State inputs:**
- `state.connection`: `"connecting" | "open" | "reconnecting" | "closed"` from the store
- `InboxModel.engineRunning`: boolean (server-sent)
- `InboxModel.engineDownSince`: ISO timestamp when engine went down (deferred to pro-d571)
- `InboxModel.engineRestartReason`: `"install"` or absent (deferred to pro-d571)
- `lastSeenTimestamp`: client-side, updated when socket goes `open` (captures last-seen time for `server-unreachable`)

## 2. HTTPS requirements by browser

This section documents the HTTPS facts for running the WebUI as a PWA on a LAN. **All LAN HTTPS provisioning is deferred to pro-d571** (and possibly pro-d591); the web/ code assumes HTTPS is already available.

### 2.1 Secure-context definition

A **secure context** is:
- HTTPS origin
- OR the `localhost` domain (`http://localhost:3333`)
- OR loopback IP (`http://127.0.0.1:3333`, `http://[::1]:3333`)
- OR file: protocol

A plain-http LAN IP (`http://192.168.1.10:3333`) is NOT a secure context; service workers will not register there.

**Source:** [W3C Secure Contexts spec](https://w3c.github.io/webappsec-secure-contexts/), [MDN Service Worker Guide](https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API/Using_Service_Workers).

### 2.2 Certificate requirements (LAN HTTPS)

For HTTPS on a LAN IP or DNS name, the certificate must:

1. **Chain to a trusted CA** on the device. mkcert generates a self-signed root and issues a leaf cert signed by that root; the root must be installed and trusted on each device.
2. **Match the host in the URL via SAN.** Common Name (CN) is ignored by modern browsers (Chrome ≥58). IP access needs an `iPAddress` SAN; DNS name access needs a `dNSName` SAN. Example: `mkcert 192.168.1.10 hostname.local localhost`.
3. **Have no certificate errors.** A cert error blocks the SW script fetch even if the user clicks through the warning (Chrome/Safari behavior).

### 2.3 mkcert provisioning (deferred to pro-d571)

**mkcert 1.4.4 or later** (`npm view mkcert` for current version):

```bash
# On the cpd host (one-time)
mkcert -install                          # Add local CA to system and browser stores
mkcert 192.168.1.10 hostname.local localhost  # Issue leaf cert + key
# Output: cert.pem, key.pem

# On other devices (manual, per device)
# macOS: Share rootCA.pem (from mkcert -CAROOT) → AirDrop → Settings → Profile → Install
# iOS: Share rootCA.pem → Files → Open in "Settings" → VPN & Device Management → Profile → Install
#      Then: Settings > General > About > Certificate Trust Settings → Enable full trust
# Android: Settings > Security > Encryption & credentials > Install a certificate > CA certificate
```

The root CA is stored in `$(mkcert -CAROOT)/rootCA.pem`; distribute it to other devices (never the key `rootCA-key.pem`).

**Certificate lifetime:** mkcert issues 2y27d certs, safe for Apple's 825-day limit.

**Source:** [mkcert README](https://github.com/FiloSottile/mkcert#readme), [Apple CA limit](https://support.apple.com/en-us/103769).

### 2.4 HTTPS by browser (LAN; deferred to pro-d571)

| Browser | HTTPS required for SW? | Localhost exception | CA install needed? | Installable without CA? |
|---|---|---|---|---|
| Chrome/Edge desktop | Yes (both SW and install) | Yes: `http://localhost:*`, `http://127.0.0.1:*` | Yes, for LAN IP | Only on localhost |
| Android Chrome | Yes (SW + install) | Only on-device loopback (not server's) | Yes (mkcert as user cert) | No, fails cert check |
| Safari macOS 14+ | For SW; "Add to Dock" (no SW) works over http | Yes: `http://localhost:*` | Only for SW | Add to Dock works without cert |
| iOS Safari, Add to Home Screen | For SW; Share → Add to Home Screen works over http | Only on-device loopback | Yes (iOS profile + full trust) | Shortcut works without cert; no SW/offline |

**Key points:**
- Plain-http LAN IPs cannot host a service worker anywhere (not a secure context).
- `localhost` and loopback IPs are exceptions on the device running the server.
- iOS/Android require mkcert CA installed before the web app will work offline (SW registration fails without it).
- iOS "Add to Home Screen" is a shortcut, not an install; it works without HTTPS but provides no offline capability.
- **Chrome-dev-only workaround:** `--unsafely-treat-insecure-origin-as-secure=http://...` flag; not for end users or iOS.

**HTTPS provisioning is deferred to pro-d571** (cert generation, device CA install, scope/path decisions for `:3333` vs `:33333`).

### 2.5 Service-worker scope rules

The service worker is registered at `/sw.js` with the default scope of `/` (the root). This scope:

- Covers all paths under `/` (e.g., `/app`, `/api/*`, `/#/route`).
- Is limited to the origin; `:3333` and `:33333` are separate origins with separate SWs and caches.
- Applies to hash routes (`/#/route`); the fragment is not part of the scope match.

**Scope response header:** To broaden the scope beyond the directory of `/sw.js`, the server response for `/sw.js` includes `Service-Worker-Allowed: /`. The `serve.ts` must set this header.

**Source:** [W3C ServiceWorker spec §4.1](https://w3c.github.io/ServiceWorker/#path-restriction), [MDN register() docs](https://developer.mozilla.org/en-US/docs/Web/API/ServiceWorkerContainer/register).

## 3. Engine banner state machine

The banner displays exactly one kind (or none), determined by the first rule that applies (in order of precedence):

| Precedence | Condition | Kind | Display |
|---|---|---|---|
| 1 | WebSocket not `open` | `reconnecting` | `"Reconnecting…"` |
| 2 | WebSocket not `open` AND `lastSeenTimestamp` set | `server-unreachable` | `"Server unreachable"` + timestamp |
| 3 | Socket `open` AND `engineRunning === false` AND `engineRestartReason !== "install"` | `engine-down` | `"Engine down"` + timestamp |
| 4 | Socket `open` AND `engineRunning === false` AND `engineRestartReason === "install"` | `engine-restarting` | `"Engine restarting from install…"` + timestamp |
| 5 | None of the above | none | (no banner) |

**Inputs:**
- `ConnectionStatus` (store): `state.connection`
- `socket open`: iff `state.connection === "open"`
- `lastSeenTimestamp`: client tracks this when socket goes `open`; captures the moment connection was lost
- `InboxModel.engineRunning`: from server's `cockpit.snapshot` or `cockpit.patch` with pane `"inbox"`
- `InboxModel.engineDownSince`: ISO timestamp (deferred to pro-d571)
- `InboxModel.engineRestartReason`: `"install"` or absent (deferred to pro-d571)

**Transitions:**
- When `state.connection` changes, re-evaluate rules 1–2.
- When `InboxModel` updates (from server), re-evaluate rules 3–4.
- No state is held; rules are re-evaluated on every store change.

**Rendering:** Full-width bar above the Shell (above all 5 zones). When kind is `none`, render nothing.

## 4. Outbox state machine

The outbox queue persists in IndexedDB. Each entry transitions through states:

```
[queued] ──(user submits or replay)──> [sending]
  ^                                         |
  |                    (5xx or network)    |
  └─────────────────────────────────────┘  |
                                            |
                         (ack/result/resolved)
                                            |
                                            ├──> (deleted, success)
                                            |
                                            └──> [failed, error set]
```

**State details:**

- **`queued`:** Entry created, awaiting send. Triggered by: user submits while offline, or replay restarts a 5xx-errored entry.
- **`sending`:** Message sent to server, awaiting response. Triggered by: replay sends a queued entry.
- **`failed`:** Server returned an error that won't retry (4xx, or 5xx with max retries). Triggered by: receive error, or 5xx timeout. Entry stays in store with `error` detail.

**Transitions:**

| From | To | Event | Action |
|---|---|---|---|
| `queued` | `sending` | Replay starts or user submits | Send message to server |
| `sending` | (deleted) | `session.submitAck`, `action.result`, or `session.dialogResolved` (any code) | Remove from store |
| `sending` | `queued` | 5xx error or network error | Reset state for retry |
| `sending` | `failed` | 4xx error (client error) or max retries exceeded | Mark failed, set error |

**Replay order:** FIFO by `createdAt` (creation timestamp). The browser guarantees order across `IndexedDB` operations (IDBTransaction semantics).

**Dedup during replay:** When sending a queued entry, use the stored `id` (clientSubmissionId, actionId, or dialogId). The server's dedup table returns the prior result if the id matches, so a resubmission of the same entry (due to crash and recovery) is idempotent.

## 5. Server deduplication (A4, pro-d591)

The server maintains a dedup table of at most **1000 entries** (LRU eviction by `createdAt`, oldest first).

**Dedup key:** `clientSubmissionId` (for `session.submit`), `actionId` (for `cockpit.action`), or `dialogId` (for `dialog.respond`).

**Dedup on arrival:**

1. Extract the id from the message.
2. Lookup in the dedup table.
3. If found, return the stored result (ack, action result, or resolved outcome) **without re-processing**.
4. If not found, process the message and store the result with the id and a timestamp.

**Stored result format:**
- **`session.submit`:** Store the `session.submitAck` message (with `clientSubmissionId`, `queued`, optional `submissionId`).
- **`cockpit.action`:** Store the `action.result` message (with `actionId`, `ok`, `output` or `error`).
- **`dialog.respond`:** Store the `session.dialogResolved` message (with `dialogId`, `outcome`).

**Eviction:** When the table exceeds 1000 entries, evict the oldest entry by timestamp before storing a new one.

**Payload idempotence:** The dedup key is the id only; a resubmission with the same id but different payload returns the prior result (the payload change is ignored). The client is responsible for ensuring payload matches when retrying the same id.

## 6. Web-server cache headers

The WebUI server must serve static assets with cache headers to enable browser caching and offline availability:

| Path | Cache-Control | Content-Type | Note |
|---|---|---|---|
| `/sw.js` | `no-cache` | `application/javascript` | Browser checks for updates on every load; must fetch but may use 304 if unchanged |
| `/index.html` | `no-cache` | `text/html; charset=utf-8` | SPA entry point; must be fresh to reflect deployments |
| `/manifest.webmanifest` | `no-cache` | `application/manifest+json` | Manifest; must be fresh |
| `/assets/*` (hashed) | `public, max-age=31536000, immutable` | (MIME type per file) | Content-addressed; safe to cache indefinitely |
| Other static files | (none set) | (MIME type per file) | Static assets; optional caching |

**Implementation:** `web/src/server/serve.ts` serves these files and must set the `Cache-Control` header in the HTTP response. Example for `/sw.js`:

```
GET /sw.js HTTP/1.1
HTTP/1.1 200 OK
Cache-Control: no-cache
Content-Type: application/javascript
Content-Length: ...
```

**Why `no-cache` for `/sw.js`:** The W3C ServiceWorker spec requires the browser to check for updates on every page load. The `no-cache` directive (not `no-store`) allows the browser to use a cached copy if unchanged (via ETag 304), but forces a revalidation fetch to check for a new version.

## 7. Assumptions

**A1:** PWA code only; LAN HTTPS/mkcert provisioning (certificate generation, device installation, iOS profile + full trust setup) is deferred to pro-d571. Service workers register only in a secure context (HTTPS or `localhost`); a plain-http LAN origin is not secure (§2.1), so registration will fail there until HTTPS is provisioned.

**A2:** The service worker is built into the production build served by `serve.ts` (`:3333`). The `:33333` preview is the Vite dev server and does not register a service worker (it serves the manifest and assets for testing, but no SW). The dev server's `/sw.js` is a stub that does nothing.

**A3:** One global engine/connection banner, rendering above the Shell. Two wire inputs: `InboxModel.engineRunning` (bool) and `engineDownSince` (ISO, deferred to pro-d571) and `engineRestartReason` (deferred to pro-d571) while the socket is open; the client's `lastSeenTimestamp` when the socket is down. Separate from session-level status (e.g., session busy state shown in the StatusBar).

**A4:** Offline queue covers composer submissions, dialog responses, and cockpit actions (types `inbox.*`, `pr.*`, `session.retier`, `plan.reject`, `context.retire`, `engine.start`). Server dedup by id (clientSubmissionId, actionId, dialogId), persisted across server restarts (at most 1000 entries, LRU eviction). Replay order is FIFO by `createdAt`.

**A5:** Electron (global `window.cpdElectron` defined) registers no service worker and shows the same banner and outbox. Electron runs cpd locally via IPC; no offline benefit from a service worker. The banner still displays connection state for parity with the browser UI.

## 8. Implementation status

| Feature | Status | Note |
|---|---|---|
| Service-worker registration | Deferred | pro-d591: implement SW + cache strategy + offline fallback |
| Outbox UI + IndexedDB store | Deferred | pro-d591: store, replay logic, UI indicators |
| Engine banner state machine | Deferred | pro-d591: add to App component, wire inputs to store |
| Cache headers (`serve.ts`) | Deferred | pro-d591: set Cache-Control on `/sw.js`, `/index.html`, `/manifest.webmanifest` |
| HTTPS provisioning (mkcert) | Deferred | pro-d571: cert generation, device CA install, iOS profile setup, DNS/IP SAN |
| `InboxModel.engineDownSince`, `engineRestartReason` | Deferred | pro-d571: add wire fields, server sends in `cockpit.snapshot`/`cockpit.patch` |
| Dedup table (server-side) | Deferred | pro-d591: Rust engine persists dedup table, applies on every message |
| `serve.ts` `Service-Worker-Allowed` header | Deferred | pro-d591: set header on `/sw.js` response (if needed for scope) |

## 9. References

- **Web/CONTRACT.md §34:** Full wire types, state machines, and assumptions.
- **res-pwa.md:** HTTPS facts, mkcert provisioning, browser support matrix (this file, §2).
- **scout-seams.md:** Exact file locations and connection seams in web/.
- [W3C Secure Contexts](https://w3c.github.io/webappsec-secure-contexts/)
- [W3C ServiceWorker spec](https://w3c.github.io/ServiceWorker/)
- [MDN Service Worker Guide](https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API/Using_Service_Workers)
- [mkcert README](https://github.com/FiloSottile/mkcert)
- [MDN PWA Guide](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps)

Server deduplication retains completed results in memory when its file cannot be written, so retrying the same action ID does not repeat the operation in that process. Results that could not be persisted may be lost on a server restart. Client reconnect handling also drains entries requeued after a pending storage write finishes.
