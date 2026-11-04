import { PATH_BEGIN, PATH_END, parseMarkedPath, resolveLoginPath } from "./env.ts";
import type { ShellRunner } from "./env.ts";

function fakeRunner(result: { stdout?: string; error?: Error }) {
  const calls: Array<{ file: string; args: readonly string[] }> = [];
  const run: ShellRunner = (file, args) => {
    calls.push({ file, args });
    return result.error ? Promise.reject(result.error) : Promise.resolve(result.stdout ?? "");
  };
  return { run, calls };
}

test("parseMarkedPath returns the text between the markers and ignores shell noise", () => {
  const out = `noise\n${PATH_BEGIN}/a:/b${PATH_END}trailing ${PATH_BEGIN}x${PATH_END}`;
  expect(parseMarkedPath(out)).toBe("/a:/b");
  expect(parseMarkedPath(`rc noise ${PATH_BEGIN}/x${PATH_END}\n`)).toBe("/x");
});

test("parseMarkedPath throws when a marker is missing", () => {
  const msg = "login shell printed no PATH marker";
  expect(() => parseMarkedPath("nothing")).toThrow(msg);
  expect(() => parseMarkedPath(`${PATH_BEGIN}/a`)).toThrow(msg);
  expect(() => parseMarkedPath(`/a${PATH_END}`)).toThrow(msg);
});

test("resolveLoginPath runs the login shell once on darwin and returns its PATH", async () => {
  const { run, calls } = fakeRunner({ stdout: `junk${PATH_BEGIN}/opt/bin:/usr/bin${PATH_END}\n` });
  const result = await resolveLoginPath({ platform: "darwin", shell: "/bin/fish", currentPath: "/cur", run });
  expect(result).toBe("/opt/bin:/usr/bin");
  expect(calls).toHaveLength(1);
  expect(calls[0]?.file).toBe("/bin/fish");
  expect(calls[0]?.args[0]).toBe("-ilc");
  const script = calls[0]?.args[1] ?? "";
  expect(script).toContain(PATH_BEGIN);
  expect(script).toContain(PATH_END);
  expect(script).toContain("$PATH");
});

test("resolveLoginPath uses /bin/zsh when no shell is set", async () => {
  const { run, calls } = fakeRunner({ stdout: `${PATH_BEGIN}/p${PATH_END}` });
  await resolveLoginPath({ platform: "darwin", shell: undefined, currentPath: undefined, run });
  expect(calls[0]?.file).toBe("/bin/zsh");
});

test("resolveLoginPath returns the current PATH without running a shell off darwin", async () => {
  const { run, calls } = fakeRunner({ stdout: "" });
  expect(await resolveLoginPath({ platform: "linux", shell: "/bin/bash", currentPath: "/cur", run })).toBe("/cur");
  expect(await resolveLoginPath({ platform: "linux", shell: undefined, currentPath: undefined, run })).toBe("");
  expect(calls).toHaveLength(0);
});

test("resolveLoginPath propagates a shell failure", async () => {
  const boom = new Error("shell failed");
  const { run } = fakeRunner({ error: boom });
  await expect(resolveLoginPath({ platform: "darwin", shell: "/bin/zsh", currentPath: "/cur", run })).rejects.toBe(boom);
});
