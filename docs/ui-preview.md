# Previewing a UI change

A prototype is a quick way to preview a visual change in the live cockpit before building the full feature.

## The loop

(hs-d504, hs-d323, hs-d341, hs-n264)

1. **Prototype.** The Lead adds a throwaway Workflow to the Plan with a Worker Job that makes the visual change. The Workflow is never opened; it is cancelled after the preview.

2. **Load.** From the Workflow's Lead worktree, run:

   ```
   sh scripts/preview-tui.sh on <worktree>
   ```

   This builds the worktree in release mode into `~/.cache/cpd-preview`, then swaps the cockpit's view panes (tree, plan, widgets, statusbar, chat slot) to run the preview build, while the engine and Leads stay on the installed cpd. The status line shows a yellow `preview: <worktree> <sha>` pill (contract §6.1.1). Only the views read the store; the preview binary cannot write.

3. **Ask.** The Lead calls `ask_user_question` to ask the User approve or reject, then does nothing else until the answer arrives. (The prototype exists only to show the change; changes are not built into main this way.)

4. **Off.** The Lead runs:

   ```
   sh scripts/preview-tui.sh off
   ```

   This puts the installed cpd back in the views. Always run it, even after a reinstall's `cpd tui --respawn`, since the preview build can stay in the views.

5. **Real implementation.** An approved change is built through the TDD template on main's code: docs, red tests that pin the exact rows, implementation, then open (hs-d341).

## Previewing the web UI

(pro-d34)

`scripts/preview-web.sh` is the sibling of `scripts/preview-tui.sh`. Where the TUI preview swaps the cockpit's views, the web preview runs the Vite dev server of a preview worktree, so the WebUI can be seen on `localhost:5173` while the installed cpd keeps the engine and the Leads.

```
sh scripts/preview-web.sh on [<worktree>] | off | status
```

- **`on [<worktree>]`** creates the git worktree on branch `preview/web` from `origin/main` when it is missing. The default path is `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}`. It links `node_modules` and `web/node_modules` from the repo when they are absent. It starts the Vite dev server, or respawns it when it is running, with `pnpm --dir <worktree> --filter cpd-web-client run dev --host` on port 5173, in the window `web` of the cockpit's work session, without the session identity variables. It builds Electron (`build:electron`) only when `CPD_PREVIEW_ELECTRON=1`. It sets the tmux option `@cpd_web_preview` to `<worktree name> <sha>` and writes `web-state` in `${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}`. It prints the command that points Electron at the server: `CPD_ELECTRON_DEV_URL=http://localhost:5173 pnpm --dir <worktree>/web run electron`. It never kills Electron.
- **`off`** respawns the window `web` on the primary checkout, unsets the option `@cpd_web_preview` and removes `web-state`.
- **`status`** prints the worktree and the sha, or `no web preview`.

`preview-web.sh` itself never runs `git checkout`, `git reset` or `git clean`. The Electron sync does a scoped `git checkout HEAD -- <files>` in the preview worktree before it fast-forwards (see [Electron app](#electron-app)). The matching scripts in `web/package.json` are `preview:on` and `preview:off`.

## Electron app

(pro-d37, pro-d38, pro-d39)

The Electron app (`web/src/electron/main.ts`) shows the same WebUI in a window. Four behaviours:

- **Load order (pro-d37).** The window loads only after `startAutoUpdate` returns and `startServices` has run, so the IPC handlers `ghostty:wasm` and `webui:send` exist before the renderer loads. Accepted cost: the window stays blank during a first-run CLI bootstrap; `start.ts` waits for it.
- **Name (pro-d38).** The app is named CPD. `productName` is `CPD` in `web/package.json` and `web/electron-builder.yml`. `app.setName('CPD')` runs before `app.whenReady()`, and the BrowserWindow title is `CPD`. `<title>CPD</title>` is in `web/electron.html` and `web/index.html`. The `electron` script patches `CFBundleName` and `CFBundleDisplayName` to `CPD` in `Electron.app/Contents/Info.plist` with `plutil` on darwin.
- **Reload on SIGUSR1 (pro-d39).** The main process reloads the renderer in place on `SIGUSR1` (`reloadIgnoringCache`, when the webContents is not destroyed). Live patches reload without closing or focusing the window.
- **Live sync (pro-d39).** `syncCheckout` (`web/src/electron/update/checkout.ts`) syncs a live preview worktree when `origin/main` advances:
  - It no longer requires the branch `main`: any branch whose HEAD is an ancestor of `origin/main` fast-forwards.
  - It no longer skips a dirty tree.
  - Before the fast-forward it runs `git checkout HEAD -- <files changed in HEAD..origin/main>`. Landed fixes replace their temporary live patches; other live patches survive.
  - A fast-forward that cannot apply (a changed file with a conflicting local edit that survived) is reported `skipped: diverged`.
  - The sync reasons are `fetch-failed` | `diverged`. The `branch` and `dirty` reasons are removed.

## Limits

A preview costs one release build of the worktree. The engine's pi chat (pi/lib/render.ts) is not reached by the view, so messages there don't appear in the preview. A branch missing a breaking migration of main is refused by the install guard; merge main first.
