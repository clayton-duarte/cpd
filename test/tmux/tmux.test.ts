import { test } from "node:test";
import assert from "node:assert/strict";
import { CpdError } from "../../src/lib/errors.ts";
import { killWindow, newWindow, sendKeys, setWindowOption, shellQuote } from "../../src/tmux/tmux.ts";
import { fakeBin } from "../helpers/index.ts";

const ORIGINAL_PATH = process.env.PATH ?? "";
const use = (script: string) => {
  const tmux = fakeBin("tmux", script);
  process.env.PATH = `${tmux.dir}:${ORIGINAL_PATH}`;
  return tmux;
};
const args = { name: "watch wf", cwd: "/lead wt", command: "cpd watch --wf wf" };

test("newWindow: spawns tmux with argv on the given socket and returns the new pane id", async () => {
  const tmux = use(`echo '%7'`);
  assert.equal(await newWindow("/tmp/tmux-1/cpd", args), "%7");
  assert.deepEqual(tmux.calls(), [["-S", "/tmp/tmux-1/cpd", "new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "watch wf", "-c", "/lead wt", "cpd watch --wf wf"]]);
});

test("newWindow: a failing tmux is tmux_failed with its stderr; output that is not a pane id is bad_tool_output", async () => {
  use(`echo 'no server running on /tmp/s' >&2; exit 1`);
  await assert.rejects(newWindow("/tmp/s", args), (e: unknown) => e instanceof CpdError && e.code === "tmux_failed" && /no server running/.test(e.message));
  use(`echo 'hello'`);
  await assert.rejects(newWindow("/tmp/s", args), (e: unknown) => e instanceof CpdError && e.code === "bad_tool_output");
});

test("setWindowOption, sendKeys and killWindow: argv on the given socket; a failing tmux is tmux_failed", async () => {
  const tmux = use("");
  await setWindowOption("/tmp/s", "%7", "remain-on-exit", "off");
  await sendKeys("/tmp/s", "%7", ["hello world"], true);
  await sendKeys("/tmp/s", "%7", ["Enter"]);
  await killWindow("/tmp/s", "%7");
  assert.deepEqual(tmux.calls(), [
    ["-S", "/tmp/s", "set-option", "-w", "-t", "%7", "remain-on-exit", "off"],
    ["-S", "/tmp/s", "send-keys", "-t", "%7", "-l", "hello world"],
    ["-S", "/tmp/s", "send-keys", "-t", "%7", "Enter"],
    ["-S", "/tmp/s", "kill-window", "-t", "%7"],
  ]);
  use(`echo "can't find pane: %7" >&2; exit 1`);
  await assert.rejects(killWindow("/tmp/s", "%7"), (e: unknown) => e instanceof CpdError && e.code === "tmux_failed" && /can't find pane/.test(e.message));
});

test("shellQuote: the shell reads the word back unchanged", async () => {
  const { execFileSync } = await import("node:child_process");
  for (const word of ["plain", "a b", "it's", "$HOME `x` \\ \"q\"", ""]) {
    assert.equal(execFileSync("sh", ["-c", `printf %s ${shellQuote(word)}`], { encoding: "utf8" }), word);
  }
});

test("real tmux: newWindow opens a detached window in the given directory (CPD_IT=1)", { skip: process.env.CPD_IT !== "1" }, async () => {
  process.env.PATH = ORIGINAL_PATH;
  const { spawnSync } = await import("node:child_process");
  const { tmpDir } = await import("../helpers/index.ts");
  const socket = `${tmpDir("cpd-tmux-")}/s`;
  const t = (...a: string[]) => spawnSync("tmux", ["-S", socket, ...a], { encoding: "utf8" });
  t("new-session", "-d", "-s", "x");
  try {
    const pane = await newWindow(socket, { name: "watch w", cwd: "/tmp", command: "sleep 30" });
    assert.match(pane, /^%\d+$/);
    assert.equal(t("display-message", "-p", "-t", pane, "#{window_name} #{pane_current_path}").stdout.trim(), "watch w /tmp");
  } finally {
    t("kill-server");
  }
});
