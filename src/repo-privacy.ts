// Repo-wide privacy tripwire patterns and matcher.
// Each pattern flags one class of content that must never appear in a tracked
// file of this public repo. See src/repo-privacy.test.ts and H11 for context.

export type PrivacyClass = 'name' | 'work' | 'path' | 'credential' | 'email';

export interface PrivacyPattern {
  regex: RegExp;
  comment: string;
  cls: PrivacyClass;
}

export const PRIVACY_PATTERNS: PrivacyPattern[] = [
  { regex: /\bclayton\b/i, comment: 'personal first name', cls: 'name' },
  { regex: /\bduarte\b/i, comment: 'personal surname', cls: 'name' },
  { regex: /missionlane|mission\s+lane/i, comment: 'employer name', cls: 'work' },
  { regex: /EEC-/, comment: 'work ticket prefix', cls: 'work' },
  { regex: /\bjira\b/i, comment: 'work tool: Jira', cls: 'work' },
  { regex: /\batlassian\b/i, comment: 'work tool: Atlassian', cls: 'work' },
  { regex: /\bslack\b/i, comment: 'work tool: Slack', cls: 'work' },
  // Only literal absolute personal paths — $HOME/ and ~/ are the correct,
  // portable way to write paths and must never trip the guard.
  { regex: /\/Users\//, comment: 'macOS home path', cls: 'path' },
  { regex: /\/home\//, comment: 'linux home path', cls: 'path' },
  { regex: /\bghp_[A-Za-z0-9]+/, comment: 'GitHub personal access token shape', cls: 'credential' },
  { regex: /\bghu_[A-Za-z0-9]+/, comment: 'GitHub user-to-server token shape', cls: 'credential' },
  { regex: /\bgithub_pat_[A-Za-z0-9_]+/, comment: 'GitHub fine-grained PAT shape', cls: 'credential' },
  { regex: /\bsk-[A-Za-z0-9]+/, comment: 'OpenAI-style secret key shape', cls: 'credential' },
  { regex: /\bBearer\s+\S+/, comment: 'bearer token header', cls: 'credential' },
  {
    regex: /api[_-]?key\s*[:=]\s*["'][^"']+["']/i,
    comment: 'quoted api key literal',
    cls: 'credential',
  },
  // Work email: any @ address that is not an obviously-public placeholder.
  // Allowed placeholders: example.com (RFC 2606) and the project's throwaway
  // identity cpd@duck.com (deliberate, appears in git metadata by design).
  {
    regex: /\b[A-Za-z0-9._%+-]+@(?!example\.com\b)(?!duck\.com\b)[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/i,
    comment: 'non-placeholder email address',
    cls: 'email',
  },
];

export interface PrivacyMatch {
  matchedText: string;
  comment: string;
  cls: PrivacyClass;
}

export function findPrivacyMatches(line: string): PrivacyMatch[] {
  const matches: PrivacyMatch[] = [];
  for (const { regex, comment, cls } of PRIVACY_PATTERNS) {
    const m = line.match(regex);
    if (m) {
      matches.push({ matchedText: m[0], comment, cls });
    }
  }
  return matches;
}

// Narrow, explicit allowlist of (file, pattern-class) pairs. Everything else
// in these files is still scanned — only the listed classes are exempt, and
// each entry says why. A blanket file skip would have let the actual leak
// (the user's first name in DECISIONS.md) sail through again; see H11/D119.
export interface AllowlistEntry {
  file: string;
  classes: PrivacyClass[];
  reason: string;
}

export const PRIVACY_ALLOWLIST: AllowlistEntry[] = [
  {
    file: 'src/fixtures/sample.test.ts',
    classes: ['work'],
    reason: 'contains the OLD tripwire regex literal naming its own vocabulary (self-reference)',
  },
  {
    file: 'DECISIONS.md',
    classes: ['work'],
    reason: 'D119 prose describes the guard\'s own vocabulary while documenting it (self-reference)',
  },
];

export function isAllowlisted(file: string, cls: PrivacyClass): boolean {
  return PRIVACY_ALLOWLIST.some((e) => e.file === file && e.classes.includes(cls));
}
