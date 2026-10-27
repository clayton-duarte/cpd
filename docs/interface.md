# cpd's interface

Generated from the `tree` of `cpd --help` by `cli/tests/interface.rs`; do not edit by hand. Regenerate: `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface`. Callers, cockpit and internal: [commands.md §2](commands.md#2-command-index). A § without a file is commands.md's.

| Command | Description | Callers | Cockpit |
|---|---|---|---|
| `cpd context` | Contexts and their records (commands.md §3) | | |
| `cpd context new` | Create a Context (§3.1) | Lead | yes |
| `cpd context list` | List the Contexts (§3.2) | Lead, Manager | |
| `cpd context show` | Show a Context and its records (§3.3) | Lead, Manager | |
| `cpd context search` | Search the Context's records, ranked by the holographic engine (§3.3) | Lead, Manager | |
| `cpd context reindex` | Rebuild the Context's holographic engine index (§3.3) | Lead, Manager | |
| `cpd context decide` | Record a decision (§3.4) | Lead, Manager | |
| `cpd context note` | Record a note (§3.4) | Lead, Manager | |
| `cpd context ask` | Record a question (§3.4) | Lead, Manager | |
| `cpd context answer` | Record the answer to a question (§3.5) | Lead, Manager | |
| `cpd context rename` | Change a Context's title (§3.7) | | yes |
| `cpd context archive` | Archive the Context with its Plans and their Sessions (§3.8) | | yes |
| `cpd context unarchive` | Unarchive the Context with its Plans and their Sessions (§3.9) | | yes |
| `cpd context manager` | Start or resume the Context's Manager (§3.10) | Manager | yes |
| `cpd context pending` | List the in-flight work of the Context's active Plans (§3.11) | Lead, Manager | |
| `cpd context distill` | Build the Context's digest with one-shot pi (§3.12) | Lead | |
| `cpd context owner` | Set or change the owner of a Context (§3.13) | Lead, Manager | |
| `cpd context delete` | Delete an archived or asleep Context: its Plans, records, Manager, files, worktrees and local branches (§3.14) | | yes |
| `cpd context curation` | List the pending retirements and documentation candidates (§3.15) | internal | |
| `cpd context retire` | Decide the pending retirements (§3.16) | internal | |
| `cpd context docs` | Decide the pending documentation candidates (§3.17) | internal | |
| `cpd plan` | Plans and their phases (commands.md §4) | | |
| `cpd plan new` | Create a Plan and start its Lead (§4.1) | Lead, Manager | yes |
| `cpd plan list` | List the Plans (§4.2) | Lead, Manager | |
| `cpd plan show` | Show a Plan: the TUI's snapshot (§4.3) | Lead, Manager | |
| `cpd plan phase` | Move the Plan to phase `plan` (§4.4) | Lead | |
| `cpd plan render` | Render the Plan to run/<plan>/plan.md (§4.5) | Lead | |
| `cpd plan review` | Request the User's approval of the draft Jobs (§4.6) | Lead | |
| `cpd plan decide` | Record the User's decision on a Plan review (§4.7) | internal | |
| `cpd plan outline` | Set the Plan's outline: what its review summary says (§4.13) | Lead | |
| `cpd plan lock` | Lock the Plan and start dispatching (§4.8) | Lead | |
| `cpd plan rename` | Change the Plan's title (§4.9) | | yes |
| `cpd plan archive` | Archive the Plan, its Lead and its side forks (§4.10) | | yes |
| `cpd plan unarchive` | Unarchive the Plan, its Lead and its side forks (§4.11) | | yes |
| `cpd plan cancel` | Cancel the Plan's Jobs and pull requests, close its Lead, archive it (§4.12) | Lead | |
| `cpd plan delete` | Delete an archived or asleep Plan: its rows, files, worktrees and local branches (§4.14) | | yes |
| `cpd job` | Jobs, Specs and Dependencies (commands.md §5) | | |
| `cpd job add` | Add a Job (§5.1) | Lead | |
| `cpd job edit` | Edit a draft Job (§5.2) | Lead | |
| `cpd job cancel` | Cancel a Job (§5.3) | Lead | |
| `cpd job retry` | Queue a failed Job again at a Tier (§5.4) | Lead | |
| `cpd job urgent` | Mark a Job urgent, only when the User asks (§5.8) | Lead, Manager | |
| `cpd job approve-tests` | The Lead approves an impl Job to change red tests (§5.9) | Lead | |
| `cpd job dispatch` | Start one Ready Job now (§5.5) | | |
| `cpd job list` | List the Jobs (§5.6) | Lead | |
| `cpd job show` | Show a Job and its Attempts (§5.7) | Lead | |
| `cpd attempt` | Attempts (commands.md §6) | | |
| `cpd attempt submit` | Submit the Attempt for review (Worker) (§6.1) | Worker | |
| `cpd attempt read` | A readable transcript of the Worker (§6.2) | Lead | |
| `cpd attempt show` | Show an Attempt (§6.3) | Lead | |
| `cpd attempt list` | List Attempts (§6.3) | Lead | |
| `cpd attempt run` | The Attempt driver (internal) (§6.4) | internal | |
| `cpd ask` | Ask the Lead a question and wait for the answer (Worker) (commands.md §8.1) | Worker | |
| `cpd answer` | Answer a Worker's Escalation (Lead) (commands.md §8.2) | Lead | |
| `cpd board` | The message board (commands.md §10a) | | |
| `cpd board post` | Post a message to a Session (it never starts a turn) | Lead, Manager | |
| `cpd board list` | List the posts to a Session | Lead, Manager | |
| `cpd board take` | Return and mark as read the unread posts to a Session (internal) | internal | |
| `cpd pr` | Pull requests (commands.md §9) | | |
| `cpd pr add` | Add a PR to the Plan (§9.4) | Lead | |
| `cpd pr edit` | Change a waiting PR before the lock (§9.5) | Lead | |
| `cpd pr describe` | Store the PR's description, which becomes the pull request body (§9.5a) | Lead | |
| `cpd pr cancel` | Remove a PR from the Plan before the lock; after it, end a PR with nothing to open, or whose work another PR carried (`--superseded-by`) (§9.6) | Lead | |
| `cpd pr start` | Start a waiting PR whose needs are merged, after a failed start (§9.7) | Lead | |
| `cpd pr open` | Push the Lead Branch and open the PR, refused only when it conflicts with its base, never as a draft (§9.1) | Lead | |
| `cpd pr poll` | Poll every open pull request of the User (§9.2) | Lead | |
| `cpd pr rebase` | Rebase the Lead Branch onto the base and push; --continue or --abort a rebase stopped at a conflict (§9.3) | Lead | |
| `cpd pr push` | Gate, then push the Lead Branch as it is, without a rebase (§9.9) | Lead | |
| `cpd pr view` | Show the PR's checks, reviews and comments (§9.8) | Lead | |
| `cpd pr reply` | Reply to an open feedback item of the PR's pull request as the User, with the cpd marker; never resolves a thread | Lead | |
| `cpd pr post` | Post the PR's pull request to Slack for review (§9.8b) | Lead | |
| `cpd pr merge` | Squash-merge a clean pull request on GitHub and delete its branch (§9.10) | Lead | yes |
| `cpd review` | Plannotator sessions a Session has open (hs-d496) | | |
| `cpd review open` | Record an open plannotator session | Lead, Manager | |
| `cpd review close` | Close one or every open plannotator session | Lead, Manager | |
| `cpd review list` | List the open plannotator sessions of a Session | Lead, Manager | |
| `cpd review submit` | Post a GitHub review (APPROVE or REQUEST_CHANGES) and, with --slack, queue its reaction (§31.1) | Lead, Manager | |
| `cpd policy` | Policies (commands.md §22) | | |
| `cpd policy show` | Show the resolved policy of a key (§22.1) | Lead, Manager, Worker | |
| `cpd mcp` | MCP servers (commands.md §24) | | |
| `cpd mcp attach` | Attach a server of this machine's policy to this Session only (§24.1) | Manager, Lead, Worker | |
| `cpd session` | Sessions and their tmux panes (commands.md §10) | | |
| `cpd session list` | List Sessions (§10.1) | Lead, Manager | |
| `cpd session show` | Show a Session (§10.2) | Lead, Manager | |
| `cpd session fork` | Fork a Lead into a parked window (§10.3) | Lead | yes |
| `cpd session open` | Bring a Lead, or a Context's Manager, into the chat slot (§10.4) | Lead, Manager | yes |
| `cpd session park` | Move the Lead or Manager in the chat slot to a parked window (§10.5) | Lead | yes |
| `cpd session close` | End a Lead's pane; its session file stays (§10.9) | Lead | yes |
| `cpd session history` | Show a Lead Session's history, read-only, in the chat slot (§10.12) | | yes |
| `cpd session archive` | Archive a Lead Session (§10.10) | | yes |
| `cpd session unarchive` | Unarchive a Lead Session (§10.11) | | yes |
| `cpd session send` | Send a message to a Lead (§10.13) | Lead, Manager | |
| `cpd session resume` | Resume a Lead or Manager blocked by the loop guard (§10.17) | | yes |
| `cpd session retier` | Move live Leads and Managers to the Tier the config names now (§10.15) | Manager | |
| `cpd session rename` | Rename a Session (§10.7) | Lead | yes |
| `cpd session state` | Report a Session's agent state (pi adapter) (§10.6) | internal | |
| `cpd session first-message` | Render a Lead's or Manager's first message now and write its file (adapter) (§10.15) | internal | |
| `cpd session retitle` | Write a Lead's or Manager's dynamic title with a one-shot pi (engine E10) (§10.14) | internal | |
| `cpd session report` | Report an adapter error to the Manager (pi adapter) (§10.18) | internal | |
| `cpd session switch-failed` | Record a failed model switch (pi adapter) (§10.18a) | internal | |
| `cpd session switched` | Record a successful model switch (pi adapter) (§10.18b) | internal | |
| `cpd session run` | The program in every Lead pane (internal; owns its terminal) (§10.8) | internal | |
| `cpd events` | Events (commands.md §11) | | |
| `cpd events list` | List Events (§11.1) | Manager | |
| `cpd events follow` | Follow Events, one per line (the one streaming command) (§11.2) | | |
| `cpd incident` | Incident mode (commands.md §25) | | |
| `cpd incident start` | Turn incident mode on (Manager) (commands.md §25.1) | Manager | |
| `cpd incident end` | Turn incident mode off (Manager) (commands.md §25.2) | Manager | |
| `cpd incident status` | Report whether incident mode is on (commands.md §25.3) | Manager, Lead, Worker | |
| `cpd inbox` | What needs you (commands.md §21) | | |
| `cpd inbox list` | List what needs you (§21.1) | | |
| `cpd inbox show` | Show one item (§21.2) | | |
| `cpd inbox snooze` | Hide an item for a while (§21.3) | | |
| `cpd inbox dismiss` | Hide an item for the rest of its life (§21.4) | | |
| `cpd inbox claim` | Hand a review-requested item to the review Context's Manager (§21.7) | | |
| `cpd watch` | The engine, one per machine (commands.md §12) | Lead | |
| `cpd tui` | Build the cockpit and start the engine (commands.md §13) | | |
| `cpd tui view` | A view in a cockpit pane (internal; owns its terminal) (§13.2) | internal | |
| `cpd tui view tree` | The session tree (left) | internal | |
| `cpd tui view plan` | The Plan of the Lead in the chat slot (centre top) | internal | |
| `cpd tui view widgets` | The Dispatch column (right) | internal | |
| `cpd tui view history` | An archived Session's history, read-only (the chat slot) | internal | |
| `cpd tui view legend` | The key legend (opened by `?` in the Dispatch column, in a tmux popup) | internal | |
| `cpd tui view logs` | The Event log (opened by the StatusBar's logs button and by an Inbox item, in a tmux popup) | internal | |
| `cpd tui view dialog` | A dialog of the kit: `confirm`, `input`, `text`, `checklist` or `select` (opened by the cockpit's actions, in a tmux popup) | internal | |
| `cpd tui view dialog confirm` | A question with two buttons | internal | |
| `cpd tui view dialog input` | A labelled text box that grows with its text | internal | |
| `cpd tui view dialog text` | Scrollable read-only text | internal | |
| `cpd tui view dialog checklist` | Entries to tick, with `[Approve]` and `[Later]`; the outcome is `{approved, ticked, unticked}` | internal | |
| `cpd tui view dialog select` | A radio list of options, with `[OK]` and `[Cancel]`; the outcome is `{confirmed, value}` | internal | |
| `cpd tui view dialog form` | A labelled text box and a labelled radio list, with `[OK]` and `[Cancel]`; the outcome is `{confirmed, value, choice}` | internal | |
| `cpd tui view chat` | The chat slot's placeholder | internal | |
| `cpd tui action` | A click on a StatusBar button, or a merge started at once (internal; prints nothing) (§13.2) | internal | |
| `cpd doctor` | Check the machine (commands.md §14.1) | | |
| `cpd config` | Print the effective configuration (commands.md §14.2) | | |
| `cpd tiers` | Print the Tiers and their valid efforts (commands.md §14.3) | Lead | |
| `cpd models` | The model lists pi can offer (commands.md §14.5) | | |
| `cpd models refresh` | Read the model lists pi can offer now (§14.5) | Manager, Lead | |
| `cpd models show` | Print the cached model lists (§14.5) | Manager, Lead | |
| `cpd install` | Install the cpd built from a checkout (commands.md §14.4) | | |
| `cpd secret` | Secrets in 1Password through op (commands.md §24) | | |
| `cpd secret get` | Get one field of an item (§24.1) | Lead, Manager, Worker | |
| `cpd secret list` | List the items of a vault (§24.2) | Lead, Manager | |
| `cpd secret create` | Create an item with one concealed field; the value is on stdin (§24.3) | Lead, Manager | |
| `cpd secret edit` | Set one field of an item; the value is on stdin (§24.4) | Lead, Manager | |
| `cpd secret delete` | Archive an item (§24.5) | Lead, Manager | |
| `cpd secret allow` | Let a Worker's Attempt read one field of one item (§24.6) | Lead | |
| `cpd secret approve` | Record the User's approval for another vault (adapter) (§24.7) | internal | |
| `cpd gate` | The Gate's leak check, after the command of `scripts/gate-run.sh` (commands.md §26) | | |
| `cpd gate finish` | Check what the run left behind, stop and remove it, report it (the Gate's leak check) | | |
| `cpd host` | Repair the cpd checkout after a skipped install (commands.md §9.11) | | |
| `cpd host approve` | Record the User's approval for one checkout repair (adapter) (§9.11) | internal | |
| `cpd host repair` | Repair the skipped cpd install (Manager) (§9.11) | Manager | |
| `cpd test` | The tests a change reaches, and named tests (commands.md §27) | | |
| `cpd test changed` | Run the Rust tests the changed files can reach (§26.1) | Manager, Lead, Worker | |
| `cpd test run` | Run the named tests (§26.2) | Manager, Lead, Worker | |
| `cpd test list` | Exit 0 only when that one test exists (§26.3) | Manager, Lead, Worker | |
| `cpd test build` | Compile the Rust tests and typecheck the TypeScript (§26.3) | Manager, Lead, Worker | |
| `cpd resource` | Repo-declared exclusive resources (commands.md §30) | | |
| `cpd resource list` | The repo's declared resources and who holds them (§30.1) | Manager, Lead, Worker | |
| `cpd resource run` | Take a resource and run a command holding it (§30.2) | Manager, Lead, Worker | |
| `cpd prune` | Remove the leftover worktrees whose work is on origin/main (commands.md §26) | | |
| `cpd janitor` | Remove the worktrees and branches of merged and cancelled PRs (commands.md §29) | | |
| `cpd tag` | Tag the commit that set the workspace version (commands.md §26.1) | Lead | |
| `cpd version` | The release plumbing (commands.md §26.2) | | |
| `cpd version sync` | Write the version derived from PROTOCOL and the migrations into Cargo.toml and Cargo.lock (commands.md §26.2) | Lead | |
