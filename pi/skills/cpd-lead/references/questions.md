# Questions to the User

Rules for every `ask_user_question` call (trial t5). The tool opens a dialog in the editor area; the User answers there.

- **A dialog holds one short question.** Tables, lists and long text go in the chat first; then the dialog asks. A `preview` is a few lines at most.
- **One call per interview round.** At most 4 questions, 2–4 options each. Header at most 16 characters. Each question ends with `?`.
- **Every option has a `description`**: one line stating the trade-off. Never author the labels `Other`, `Type something.` or `Next` (reserved). The tool always adds a `Type something.` row, so the User can answer freely.
- **`multiSelect: true` only when the picks are independent.** A `preview` goes on single-select options only, and stays to a few lines (the box is capped at 20 rows and does not scroll). It is not a place to read a table or a Plan; the Plan goes through `cpd_plan_review`.
- **Ask the whole frontier at once.** Put every question whose answer does not depend on another open question into the same round. A dependent question waits for the next round.
- **Do not ask what you can look up.** Read the repo and the User's material first. Ask for decisions, not facts.
- **Every open decision is a round.** Ask it at any phase, also while the Plan has draft Jobs. Put the recommended option first, its description starting `Recommended:`.
- **Never approve a Plan with this tool.** Plan approval is `cpd_plan_review`: it shows the summary in chat and the User decides in its dialog.

## Reading the result

- Answers are in `details.answers`: `answer` for a single-select, `selected` (a list) for a multi-select.
- Per-question notes arrive inside the result text after `user notes:`; the global note after `global note:`. Treat a note as part of the answer.
- `details.cancelled === true`, or the text "User declined to answer questions", means the User declined: stop and ask the same thing in chat.

## Call shapes

Interview round:

```ts
ask_user_question({questions: [
  {header: "Storage", question: "Where should the cache live?",
   options: [
     {label: "SQLite", description: "One file, transactional; adds a native dependency"},
     {label: "JSON file", description: "No dependency; rewrites the whole file on every write"}]},
  {header: "Clients", question: "Which clients must v0 support?", multiSelect: true,
   options: [
     {label: "CLI", description: "Scriptable; no UI work"},
     {label: "HTTP API", description: "Needed by the web app; adds auth scope"},
     {label: "Web UI", description: "Largest Job; depends on the HTTP API"}]}]})
```

Decision round (the recommended option first; the User may still type another answer):

```ts
ask_user_question({questions: [
  {header: "Retries", question: "How should a failed upload retry?",
   options: [
     {label: "Backoff", description: "Recommended: three tries with growing delay; fewest duplicate uploads"},
     {label: "Fixed", description: "One retry after 5 s; simplest, fails more often"},
     {label: "None", description: "Report the failure at once; the caller retries"}]}]})
```

Assumptions (state them in the question, as text or a preview; the preview holds at most a few lines, or only the count; too long for 1500 characters: write them to a file outside the repo and point to it in chat):

```ts
ask_user_question({questions: [{
  header: "Assumptions", question: "Accept these assumptions?",
  options: [
    {label: "Accept", description: "Write them into the Specs", preview: "A1 Node 22 only\nA2 No schema migration\nA3 Keep the public API"},
    {label: "Change", description: "Press n to say which to change", preview: "Change"}]}]})
```

If the User picks Change, apply their notes, restate the assumptions and ask again.
