# WebUI network ports and origins

Decided by pro-d541 (ports), pro-d542 (port in use), pro-d543 (LAN origins) and pro-d552 (browser entry on 3333, IPv6 ULA rule, b19). The normative origin and port rules are in `web/CONTRACT.md` §15.1; the desktop window is in `web/ELECTRON.md`, "WebUI ports". This page states the port map and the browser entry.

## Port map

| Port | Server | Serves | Bind |
|---|---|---|---|
| 3333 | production WebUI server (`web/src/server/serve.ts`) | the browser build from `web/dist` (entry `index.html`, which loads `main.tsx`), and `/cpd-ws` on the same port and origin | `0.0.0.0` |
| 33333 | preview Vite dev server | the same WebSocket endpoint `/cpd-ws` on its httpServer, with `strictPort` | `0.0.0.0` |

A server that must bind 3333 or 33333 while another process holds it exits non-zero, names the holder, and never falls back to another port (pro-d542; `web/CONTRACT.md` §15.1).

## Production server: browser entry on 3333

- The production server on `0.0.0.0:3333` serves the browser build. Its static root is `web/dist`; `GET /` returns `index.html`, which loads the browser entry `main.tsx`.
- `/cpd-ws` is served on port 3333, same origin as the page. The browser entry connects to it without any Electron bridge: it does not use the preload `contextBridge` (`window.cpdElectron`, `webSocketUrl`).
- CPD.app is a different entry. It keeps `electron.html`, loaded by the desktop main process with `loadFile`. The browser entry and `electron.html` are not interchangeable.

## Origin verification

The endpoint `/cpd-ws` accepts a connection before any hub is created when its `Origin` is absent, `null`, `file://`, or an accepted host on port 3333 or 33333 with scheme `http` (`web/CONTRACT.md` §15.1 for the full rule). Accepted hosts are `localhost`, loopback (`127.0.0.1`, `[::1]`), RFC 1918 IPv4, IPv6 ULA, and `.local` names.

### IPv6 ULA

An IPv6 ULA Origin (`fc00::/7`) is accepted only when its host is an IPv6 address literal in brackets, for example `[fd12:3456::1]`, parsed as an address, whose first 7 bits are `1111110`.

A hostname that only begins with `fc` or `fd` is not an address literal and is rejected. Examples: `http://fd.evil.com:3333`, `http://fc-attacker.example:33333`. The host is never matched on its text prefix.

Status: decided. As of this document the code does not do this: `serve.ts` serves `electron.html` from `dist-electron/renderer`, and `ws.ts` `isIpv6Ula` accepts any host starting with `fc` or `fd` (`web/src/server/ws.ts:110-116`, `:166`). The implementing change brings the code in line.

## Not an access control

The Origin check is not access control. A browser sets `Origin`; a non-browser client on the LAN can send any `Origin`. There is no token and no TLS, by decision (pro-d543).
