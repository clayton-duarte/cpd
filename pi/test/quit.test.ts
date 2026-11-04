/**
 * The /quit marker (decision pro-d361): the adapter installs an editor wrapper that writes the marker file at
 * `CPD_QUIT_MARKER` when the User submits exactly `/quit` (trimmed), then lets the original submit run. pi copies its own
 * `onSubmit` onto a custom editor after `setEditorComponent`, so the wrapper must intercept an assigned `onSubmit`.
 */
import "./compact-tools/pinned-themes.ts";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { visibleWidth } from "@earendil-works/pi-tui";
import { hookEditor, setWorkingWords } from "../compact-tools/extensions/compact-tools-editor.ts";
import { installQuitMarker } from "../lib/quit.ts";

type Editor = { onSubmit?: (text: string) => void };
type Factory = (tui: unknown, theme: unknown, keybindings: unknown) => Editor;

const TUI = { requestRender: () => {}, terminal: { columns: 80, rows: 24 } };
const THEME = { borderColor: (s: string) => s, selectList: {} };

/** Installs against a ctx that records the factory; `env` is the host's environment. */
function install(env: Record<string, string | undefined>): Factory[] {
  const factories: Factory[] = [];
  const ctx = { ui: { setEditorComponent: (f: Factory) => factories.push(f) } };
  installQuitMarker(ctx as never, { env } as never);
  return factories;
}

/** The wrapped editor with `original` assigned as pi does, and the dir the marker lives in. */
function setup(t: { after: (f: () => void) => void }) {
  const dir = mkdtempSync(join(tmpdir(), "quit-marker-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const marker = join(dir, "run", "quit", "s1");
  const [factory] = install({ CPD_QUIT_MARKER: marker });
  assert.ok(factory, "an editor factory is installed");
  const editor = factory(TUI, THEME, {});
  const submitted: string[] = [];
  editor.onSubmit = (text) => submitted.push(text);
  return { editor, marker, submitted };
}

for (const text of ["/quit", " /quit "]) {
  test(`submitting ${JSON.stringify(text)} writes the marker and still submits`, (t) => {
    const { editor, marker, submitted } = setup(t);
    assert.equal(existsSync(marker), false);
    editor.onSubmit?.(text);
    assert.equal(existsSync(marker), true, "the marker file is written");
    assert.deepEqual(submitted, [text], "the original submit ran");
  });
}

for (const text of ["hello", "/quitx", "/quit now"]) {
  test(`submitting ${JSON.stringify(text)} writes no marker but still submits`, (t) => {
    const { editor, marker, submitted } = setup(t);
    editor.onSubmit?.(text);
    assert.equal(existsSync(marker), false, "no marker file");
    assert.deepEqual(submitted, [text], "the original submit ran");
  });
}

test("without CPD_QUIT_MARKER nothing is installed", () => {
  assert.deepEqual(install({}), []);
});

test("installQuitMarker editor embeds the working status", (t) => {
  const { editor } = setup(t);
  assert.equal((editor as { embedWorkingStatus?: boolean }).embedWorkingStatus, true);
});

test("installQuitMarker editor draws the working pill in the top border", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "quit-marker-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const [factory] = install({ CPD_QUIT_MARKER: join(dir, "run", "quit", "s1") });
  assert.ok(factory, "an editor factory is installed");
  hookEditor();
  setWorkingWords(undefined, 0);
  t.after(() => setWorkingWords(undefined));
  const ui = { requestRender: () => {}, terminal: { rows: 40, columns: 60 } };
  const editor = factory(ui, { borderColor: (s: string) => s }, { matches: () => false }) as unknown as {
    workingStatusIndicator?: unknown;
    render(width: number): string[];
  };
  editor.workingStatusIndicator = { kind: "compaction" };
  const row = editor.render(60)[0]!.replace(/\x1b\[[0-9;]*m/gu, "");
  assert.match(row, /^╭─\ue0b6.*Compacting….*\ue0b4─+╮$/u);
  assert.equal(visibleWidth(row), 60);
});
