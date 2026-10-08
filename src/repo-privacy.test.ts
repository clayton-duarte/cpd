import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { PRIVACY_PATTERNS, findPrivacyMatches, isAllowlisted } from './repo-privacy';

describe('findPrivacyMatches (matcher unit tests)', () => {
  it('flags a personal name', () => {
    expect(findPrivacyMatches('Clayton was here').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('surname Duarte appears').length).toBeGreaterThan(0);
  });

  it('flags employer / work context', () => {
    expect(findPrivacyMatches('missionlane internal').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('Mission Lane project').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('ticket EEC-1234').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('filed in jira').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('hosted on atlassian').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('posted to slack').length).toBeGreaterThan(0);
  });

  it('flags literal absolute home paths', () => {
    expect(findPrivacyMatches('see /Users/someone/x').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('path /home/someone/y').length).toBeGreaterThan(0);
  });

  it('does not flag portable $HOME or ~ paths', () => {
    expect(findPrivacyMatches('export VAR=$HOME/secret').length).toBe(0);
    expect(findPrivacyMatches('see $HOME/.pi/agent/auth.json').length).toBe(0);
    expect(findPrivacyMatches('see ~/agent/auth.json').length).toBe(0);
  });

  it('flags credential shapes', () => {
    expect(findPrivacyMatches('token ghp_AAAABBBBCCCCDDDD').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('token ghu_AAAABBBBCCCCDDDD').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('token github_pat_AAAABBBBCCCC').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('key sk-AAAABBBBCCCCDDDD').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('Authorization: Bearer ***').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('api_key = "abcdef12345"').length).toBeGreaterThan(0);
    expect(findPrivacyMatches('apiKey: "abcdefghijklmno"').length).toBeGreaterThan(0);
  });

  it('flags non-placeholder work email addresses', () => {
    expect(findPrivacyMatches('contact someone@realcompany.com').length).toBeGreaterThan(0);
  });

  it('does not flag the throwaway project identity', () => {
    expect(findPrivacyMatches('git author cpd@duck.com').length).toBe(0);
  });

  it('does not flag an example.com placeholder email', () => {
    expect(findPrivacyMatches('contact dev@example.com').length).toBe(0);
  });

  it('does not flag innocuous text', () => {
    expect(findPrivacyMatches('the quick brown fox jumps').length).toBe(0);
  });

  it('exports patterns with a comment and class per entry', () => {
    expect(PRIVACY_PATTERNS.length).toBeGreaterThan(0);
    for (const p of PRIVACY_PATTERNS) {
      expect(p.comment.length).toBeGreaterThan(0);
      expect(p.regex).toBeInstanceOf(RegExp);
      expect(p.cls.length).toBeGreaterThan(0);
    }
  });
});

describe('isAllowlisted (allowlist is part of the guard and must be tested)', () => {
  it('exempts sample.test.ts for the work class only', () => {
    expect(isAllowlisted('src/fixtures/sample.test.ts', 'work')).toBe(true);
  });

  it('exempts DECISIONS.md for the work class only', () => {
    expect(isAllowlisted('DECISIONS.md', 'work')).toBe(true);
  });

  it('does NOT exempt DECISIONS.md for name, path, credential or email classes', () => {
    expect(isAllowlisted('DECISIONS.md', 'name')).toBe(false);
    expect(isAllowlisted('DECISIONS.md', 'path')).toBe(false);
    expect(isAllowlisted('DECISIONS.md', 'credential')).toBe(false);
    expect(isAllowlisted('DECISIONS.md', 'email')).toBe(false);
  });

  it('does not exempt an unrelated file', () => {
    expect(isAllowlisted('README.md', 'work')).toBe(false);
  });
});

describe('repo-wide privacy tripwire (tracked files)', () => {
  it('scans every tracked, plausibly-text file and finds no non-allowlisted matches', () => {
    const repoRoot = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
    const files = execFileSync('git', ['ls-files'], { cwd: repoRoot, encoding: 'utf8' })
      .split('\n')
      .filter(Boolean);

    const selfPath = path.relative(repoRoot, path.resolve(__dirname, './repo-privacy.test.ts')).split(path.sep).join('/');
    const matcherPath = path.relative(repoRoot, path.resolve(__dirname, './repo-privacy.ts')).split(path.sep).join('/');

    const BINARY_EXTENSIONS = new Set([
      '.png', '.jpg', '.jpeg', '.gif', '.ico', '.woff', '.woff2', '.ttf', '.eot',
      '.pdf', '.zip', '.tar', '.gz', '.lock',
    ]);
    // repo-privacy.ts/.test.ts are self-reference: they necessarily contain
    // every pattern class. Everything else is scanned normally; the narrow
    // (file, class) allowlist in repo-privacy.ts covers the other two cases.
    const SKIP_FILES = new Set(['pnpm-lock.yaml', selfPath, matcherPath]);

    const failures: string[] = [];

    for (const file of files) {
      if (SKIP_FILES.has(file)) continue;
      const ext = path.extname(file).toLowerCase();
      if (BINARY_EXTENSIONS.has(ext)) continue;

      const absPath = path.join(repoRoot, file);
      let content: string;
      try {
        content = readFileSync(absPath, 'utf8');
      } catch {
        continue; // unreadable / not utf8-plausible-text — skip
      }
      // Heuristic: if the decoded content contains a NUL byte, treat as binary.
      if (content.includes('\u0000')) continue;

      const lines = content.split('\n');
      lines.forEach((line, idx) => {
        const matches = findPrivacyMatches(line);
        for (const m of matches) {
          if (isAllowlisted(file, m.cls)) continue;
          failures.push(`${file}:${idx + 1}: matched "${m.matchedText}" (${m.comment})`);
        }
      });
    }

    expect(failures, failures.join('\n')).toEqual([]);
  });
});
