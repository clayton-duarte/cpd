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

## Limits

A preview costs one release build of the worktree. The engine's pi chat (pi/lib/render.ts) is not reached by the view, so messages there don't appear in the preview. A branch missing a breaking migration of main is refused by the install guard; merge main first.
