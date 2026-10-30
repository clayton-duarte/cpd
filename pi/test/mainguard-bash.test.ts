import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { primaryCheckoutMove } from "../lib/mainguard.ts";

// commands.md §15.16, the bash-write half: no agent changes the files of a repo's primary checkout.

type Fx = { tmp: string; primary: string; wt: string; inside: string; plain: string; missing: string };

/** The file system of §15.16: a primary checkout (`.git` directory), a linked worktree beside it, a linked worktree inside it, a plain directory and a missing path; no git runs. */
function fixtures(t: TestContext): Fx {
  const tmp = mkdtempSync(join(tmpdir(), "mainguard-bash-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "src"));
  mkdirSync(join(primary, "docs"));
  writeFileSync(join(primary, "docs", "commands.md"), "x");
  writeFileSync(join(primary, "f"), "x");
  const wt = join(tmp, "projects", "repo.wt");
  mkdirSync(join(wt, "src"), { recursive: true });
  writeFileSync(join(wt, ".git"), "gitdir: ../repo/.git/worktrees/repo.wt\n");
  const inside = join(primary, "inside");
  mkdirSync(join(inside, "src"), { recursive: true });
  writeFileSync(join(inside, ".git"), "gitdir: ../../repo/.git/worktrees/inside\n");
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  return { tmp, primary, wt, inside, plain, missing: join(tmp, "does-not-exist") };
}

/** Judge `command` from `cwd` (the plain directory unless a case says otherwise); the temporary directory is `home`. */
const at = (x: Fx, command: string, cwd = x.plain) => primaryCheckoutMove(command, cwd, x.tmp);

test("refuses a bash redirect into a primary checkout, with an fd prefix, append, clobber and the ampersand form", (t) => {
  const x = fixtures(t);
  const f = join(x.primary, "f");
  const err = join(x.primary, "err");
  const cwdPrimary = `cat > docs/x.md`;
  for (const [command, want] of [
    [`echo x > ${f}`, `> ${f} in ${x.primary}`],
    [`echo x >> ${f}`, `>> ${f} in ${x.primary}`],
    [`echo x >| ${f}`, `>| ${f} in ${x.primary}`],
    [`cmd 2> ${err}`, `> ${err} in ${x.primary}`],
    [`cmd 2>${err}`, `> ${err} in ${x.primary}`],
    [`cmd &> ${f}`, `> ${f} in ${x.primary}`],
    [`cd ${x.primary} && echo x >> NOTES.md`, `>> ${join(x.primary, "NOTES.md")} in ${x.primary}`],
    [`cat <<EOF > ${f}`, `> ${f} in ${x.primary}`],
  ] as const) assert.equal(at(x, command), want, command);
  assert.equal(at(x, cwdPrimary, x.primary), `> ${join(x.primary, "docs", "x.md")} in ${x.primary}`, cwdPrimary);
});

test("passes a redirect that duplicates a descriptor, one to dev null, one outside a primary checkout and quoted text", (t) => {
  const x = fixtures(t);
  const f = join(x.primary, "f");
  for (const command of [
    "cmd 2>&1",
    "cmd > /dev/null 2>&1",
    `echo x > ${join(x.plain, "f")}`,
    `echo x > ${join(x.wt, "f")}`,
    `cat < ${f}`,
    `cat ${f} > ${join(x.plain, "out")}`,
    "echo 'a > b'",
    "echo x > $OUT",
  ] as const) {
    const cwd = command === "cmd > /dev/null 2>&1" ? x.primary : x.plain;
    assert.equal(at(x, command, cwd), undefined, command);
  }
});

test("refuses tee, sed in place, perl in place, rm, touch, truncate, dd and patch aimed at a primary checkout", (t) => {
  const x = fixtures(t);
  const f = join(x.primary, "f");
  const src = join(x.primary, "src");
  const doc = join(x.primary, "docs", "commands.md");
  for (const [command, want] of [
    [`echo x | tee ${f}`, `tee ${f} in ${x.primary}`],
    [`echo x | tee -a ${f}`, `tee ${f} in ${x.primary}`],
    [`sed -i s/a/b/ ${doc}`, `sed ${doc} in ${x.primary}`],
    [`sed -i.bak s/a/b/ ${doc}`, `sed ${doc} in ${x.primary}`],
    [`sed --in-place s/a/b/ ${doc}`, `sed ${doc} in ${x.primary}`],
    [`sed -ni s/a/b/ ${doc}`, `sed ${doc} in ${x.primary}`],
    [`perl -pi -e s/a/b/ ${f}`, `perl ${f} in ${x.primary}`],
    [`rm ${f}`, `rm ${f} in ${x.primary}`],
    [`rm -rf ${src}`, `rm ${src} in ${x.primary}`],
    [`/usr/bin/rm ${f}`, `rm ${f} in ${x.primary}`],
    [`sudo rm ${f}`, `rm ${f} in ${x.primary}`],
    [`FOO=1 rm ${f}`, `rm ${f} in ${x.primary}`],
    [`touch ${f}`, `touch ${f} in ${x.primary}`],
    [`truncate -s 0 ${f}`, `truncate ${f} in ${x.primary}`],
    [`dd if=/dev/zero of=${f}`, `dd ${f} in ${x.primary}`],
    [`cd ${x.primary} && patch -p1 < /tmp/x.diff`, `patch ${x.primary} in ${x.primary}`],
    [`patch -d ${x.primary} -p1`, `patch ${x.primary} in ${x.primary}`],
  ] as const) assert.equal(at(x, command), want, command);
  assert.equal(at(x, `sed s/a/b/ ${f}`), undefined);
});

test("judges only the destination of cp, install and ln and every operand of mv", (t) => {
  const x = fixtures(t);
  const a = join(x.primary, "a");
  const b = join(x.primary, "b");
  const link = join(x.primary, "link");
  assert.equal(at(x, `cp ${a} ${join(x.plain, "b")}`), undefined);
  assert.equal(at(x, `ln -s ${a} ${join(x.plain, "link")}`), undefined);
  assert.equal(at(x, `cp ${join(x.plain, "a")} ${b}`), `cp ${b} in ${x.primary}`);
  assert.equal(at(x, `cp -t ${x.primary} ${join(x.plain, "a")}`), `cp ${x.primary} in ${x.primary}`);
  assert.equal(at(x, `cp --target-directory=${x.primary} ${join(x.plain, "a")}`), `cp ${x.primary} in ${x.primary}`);
  assert.equal(at(x, `install -m 644 ${join(x.plain, "a")} ${b}`), `install ${b} in ${x.primary}`);
  assert.equal(at(x, `ln -s ${join(x.plain, "a")} ${link}`), `ln ${link} in ${x.primary}`);
  assert.equal(at(x, `mv ${a} ${join(x.plain, "b")}`), `mv ${a} in ${x.primary}`);
  assert.equal(at(x, `mv ${join(x.plain, "a")} ${b}`), `mv ${b} in ${x.primary}`);
});

test("passes commands that only read a primary checkout", (t) => {
  const x = fixtures(t);
  const doc = join(x.primary, "docs", "commands.md");
  const f = join(x.primary, "f");
  const src = join(x.primary, "src");
  for (const command of [
    `cat ${doc}`,
    `grep -rn x ${src}`,
    `sed -n 1,5p ${f}`,
    `ls ${x.primary}`,
    `head -5 ${f} | wc -l`,
    `git -C ${x.primary} show origin/main:README.md`,
    `cd ${x.primary} && cat f`,
    `echo rm ${f}`,
    `grep 'rm ${x.primary}' f`,
  ] as const) assert.equal(at(x, command), undefined, command);
});

test("judges a glob target by its literal directory and does not judge one it cannot resolve", (t) => {
  const x = fixtures(t);
  assert.equal(at(x, `rm ${join(x.primary, "docs")}/*.md`), `rm ${join(x.primary, "docs")} in ${x.primary}`);
  assert.equal(at(x, `rm ${x.primary}/*`), `rm ${x.primary} in ${x.primary}`);
  for (const command of [
    `rm ${join(x.plain, "docs")}/*.md`,
    "rm $DIR/f",
    `touch ${x.primary}/$NAME`,
    "rm ~someone/f",
    'rm "$(mktemp -d)/f"',
    "cd $(mktemp -d) && rm f",
  ] as const) assert.equal(at(x, command), undefined, command);
  assert.equal(at(x, `cd $(mktemp -d) && cd ${x.primary} && rm f`), `rm ${join(x.primary, "f")} in ${x.primary}`);
});

const SEVEN = ["checkout", "switch", "reset", "rebase", "merge", "pull", "stash"] as const;
const TEN = ["restore", "clean", "commit", "cherry-pick", "revert", "am", "apply", "rm", "mv", "add"] as const;

test("refuses restore, clean, commit, cherry-pick, revert, am, apply, rm, mv and add in a primary checkout", (t) => {
  const x = fixtures(t);
  for (const s of TEN) {
    const want = `git ${s} in ${x.primary}`;
    assert.equal(at(x, `cd ${x.primary} && git ${s} x`), want, s);
    assert.equal(at(x, `git -C ${x.primary} ${s} x`), want, s);
  }
  assert.equal(at(x, "git clean -n", x.primary), `git clean in ${x.primary}`);
  assert.equal(at(x, "git apply --check p", x.primary), `git apply in ${x.primary}`);
  for (const s of SEVEN) assert.equal(at(x, `cd ${x.primary} && git ${s}`), `git ${s} in ${x.primary}`, s);
});

test("passes the other git commands in a primary checkout and every refused one in a linked worktree", (t) => {
  const x = fixtures(t);
  for (const r of [
    "fetch origin",
    "log origin/main",
    "show origin/main:README.md",
    "diff origin/main",
    "status",
    "branch",
    "tag v1",
    "config user.name x",
    "update-ref refs/x HEAD",
    "merge-base HEAD origin/main",
    "ls-files",
    "worktree add $(mktemp -d) origin/main",
  ] as const)
    assert.equal(at(x, `git -C ${x.primary} ${r}`), undefined, r);
  for (const s of [...SEVEN, ...TEN])
    for (const dir of [x.wt, x.inside, x.plain] as const) {
      assert.equal(at(x, `cd ${dir} && git ${s} x`), undefined, `${dir} ${s}`);
      assert.equal(at(x, `git -C ${dir} ${s} x`), undefined, `${dir} ${s}`);
    }
});

test("a heredoc body is not read as a git command or a write, the line of the operator and what follows are", (t) => {
  const x = fixtures(t);
  assert.equal(at(x, "cat <<EOF\ngit checkout main\n> f\nEOF", x.primary), undefined);
  assert.notEqual(at(x, "cat > f <<EOF\nbody\nEOF", x.primary), undefined);
  assert.notEqual(at(x, "cat <<EOF\nx\nEOF\ngit checkout main", x.primary), undefined);
});
