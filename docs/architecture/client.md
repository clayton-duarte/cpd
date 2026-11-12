# Client: the browser is the interface

Status: **decided** by pro-d567 ("per-host cpd via browser; retire CPD.app; manual WebUI/CLI upgrade; no login; pi works without cpd"). Where this page and another page disagree, the later decision stands. As of this document the code still has the CPD.app shell and its release tooling (§4); the implementing change removes them. This page states the decision, not the code's state.

## 1. Decisions in one place

1. **CPD.app is retired** (pro-d567). The Electron app is no longer a client. cpd does not build it, release it, install it into `/Applications`, or launch it.
2. **The browser is the official interface** (pro-d567). A User reaches a host's cpd from a browser: the production WebUI on port 3333, or the UI-only preview on 33333 (§2).
3. **Each host serves its own store, with no login** (pro-d567). No login, token or TLS is added. The Origin rule of `web/CONTRACT.md` §15.1 limits which pages may connect; it is not access control.
4. **Plain pi works without cpd** (pro-d567). When cpd is down or being repaired, pi runs on its own. The repair path does not depend on cpd.
5. **Upgrade is manual** (pro-d567). Nothing installs a new build on its own. The User runs the upgrade (`docs/architecture/upgrade.md`).

## 2. The browser access model

| Surface | Address | Served by | Scope |
|---|---|---|---|
| Production WebUI | `http://<host>:3333/` | the host's `cpd` engine process (`docs/architecture/serve.md`) | the host's own store |
| Engine WebSocket | `ws://<host>:3333/cpd-ws` | the same process | the core engine (`docs/contract.md` §5.8) |
| UI-only preview | `http://<host>:33333/` | the preview of `docs/architecture/serve.md` §5 | forwards `/cpd-ws` to the host's 3333; opens no store |

- **Who reaches it:** a browser on the LAN or on localhost. Hosts are separate; a browser reaches one host's cpd by that host's name. A host serves only its own store.
- **Ports are fixed.** 3333 and 33333 are not configurable (`docs/architecture/webui-network-ports.md`).
- **No login.** Access control is the network, not the application (pro-d567).

## 3. What is retired

- **CPD.app**, the Electron shell as a client: its build, its `/Applications` install and its launch.
- **The shell release:** the `app-v<version>` GitHub Release, `scripts/release-shell.sh` and `cpd desktop release` (`docs/commands.md` §14.4b).
- **The shell install:** `cpd install` downloading the shell release into `/Applications` (`core/src/install/shell.rs`), and the shell form of `cpd desktop` (`docs/commands.md` §14.4a).
- **The Electron build in `cpd install`:** the Electron step (`docs/commands.md` §14.4, step 7a). It goes with the code removal.

## 4. What the code still does

Until the implementing change lands, these still exist. Each is removed by that change; none is a current client.

- `cpd install` runs the Electron step (`docs/commands.md` §14.4, step 7a).
- `scripts/release-shell.sh` builds, signs and publishes the shell; `cpd desktop release` runs `gh release create app-v0.1.0`.
- `core/src/install/shell.rs` downloads `app-v0.1.0` into `/Applications`.

## 5. What stays

- **The WebUI.** `cpd install` builds the browser build, `web/dist`, which the production server serves over HTTP to browsers.
- **The engine and the adapter.** The engine serves the WebUI and runs the tick loop. Plain pi, without cpd, stays runnable (§1, decision 4).
- **Upgrade by command or by button**, manually, as `docs/architecture/upgrade.md` defines.

## 6. Open points

- The implementing change that removes §3 and §4 is not named in this document.
- `docs/architecture/webui-supervision.md`, `web/ELECTRON.md` and `docs/implementing.md` ("The shell release and the payload") still describe CPD.app. They are not changed here.
