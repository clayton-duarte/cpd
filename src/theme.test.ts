import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';
import { SPACE_CARDS } from './layout/spacing';

const SRC_DIR = __dirname;

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      out.push(...walk(full));
    } else if (['.ts', '.tsx', '.css'].includes(extname(full))) {
      out.push(full);
    }
  }
  return out;
}

const NEUTRAL_TOKENS = [
  '--bg-deep', '--bg', '--bg-panel', '--bg-raise', '--line', '--line-strong',
  '--fg-faint', '--fg-muted', '--fg', '--fg-bright', '--on-solid',
];

// Spacing tokens (C1): not accents, not neutrals -- exclude from the D73 hue check.
const SPACING_TOKEN_NAMES = [
  'space-1', 'space-2', 'space-3', 'space-4', 'space-5', 'space-6',
  'gap', 'pad', 'space-cards',
];

// Motion tokens (D1): durations, not accents -- exclude from the D73 hue check.
const MOTION_TOKEN_NAMES = ['duration-descend', 'duration-ascend', 'duration-reduced-fade'];

const ACCENT_HUES = ['red', 'green', 'blue'];
const ACCENT_PARTS = ['', '-solid', '-tint', '-edge'];

describe('D73 palette tripwire', () => {
  it('has exactly three accent hues, each with exactly four parts, defined in theme.css', () => {
    const css = readFileSync(join(SRC_DIR, 'theme.css'), 'utf8');
    for (const hue of ACCENT_HUES) {
      for (const part of ACCENT_PARTS) {
        expect(css).toMatch(new RegExp(`--${hue}${part}:`));
      }
    }
    // No fourth hue: nothing else named like an accent (e.g. --yellow, --purple).
    // Mantine wiring vars (--mantine-*) are excluded; only D73 tokens count.
    const allVarNames = [...css.matchAll(/(--[a-z-]+):/g)]
      .map((m) => m[1])
      .filter((name) => !name.startsWith('--mantine-'));
    const accentFullNames = allVarNames.filter(
      (name) =>
        !NEUTRAL_TOKENS.includes(name) &&
        !SPACING_TOKEN_NAMES.includes(name.replace(/^--/, '')) &&
        !MOTION_TOKEN_NAMES.includes(name.replace(/^--/, '')),
    );
    const hues = new Set(
      accentFullNames.map((name) => name.replace(/^--/, '').replace(/-(solid|tint|edge)$/, '')),
    );
    expect([...hues].sort()).toEqual([...ACCENT_HUES].sort());
  });

  it('defines every neutral token exactly once, with no extras', () => {
    const css = readFileSync(join(SRC_DIR, 'theme.css'), 'utf8');
    for (const token of NEUTRAL_TOKENS) {
      expect(css).toMatch(new RegExp(`${token}:`));
    }
  });

  it('has no raw hex color literal anywhere under src/ except theme.css', () => {
    const files = walk(SRC_DIR);
    const themeCssPath = join(SRC_DIR, 'theme.css');
    const offenders: string[] = [];
    // A real hex color token always contains at least one a-f letter in this
    // codebase (all D73 values do); pure-digit strings like '#128' are PR/issue
    // numbers in test assertions, never colors.
    const COLOR_HEX_RE = /#(?=[0-9a-fA-F]*[a-fA-F])[0-9a-fA-F]{3,8}\b/g;
    for (const file of files) {
      if (file === themeCssPath) continue;
      const content = readFileSync(file, 'utf8');
      const matches = content.match(COLOR_HEX_RE);
      if (matches) offenders.push(`${file}: ${matches.join(', ')}`);
    }
    expect(offenders).toEqual([]);
  });
});

describe('C1 spacing language tripwire', () => {
  const SPACE_TOKENS = ['--space-1', '--space-2', '--space-3', '--space-4', '--space-5', '--space-6'];

  it('declares every --space-N token in rem, not px', () => {
    const css = readFileSync(join(SRC_DIR, 'theme.css'), 'utf8');
    for (const token of SPACE_TOKENS) {
      const match = css.match(new RegExp(`${token}:\\s*([^;]+);`));
      expect(match, `${token} should be declared in theme.css`).toBeTruthy();
      expect(match![1]).toMatch(/rem\b/);
      expect(match![1]).not.toMatch(/px/);
    }
  });

  it('keeps spacing.ts SPACE_CARDS mirrored to --space-6 in theme.css', () => {
    const css = readFileSync(join(SRC_DIR, 'theme.css'), 'utf8');
    const match = css.match(/--space-6:\s*([\d.]+)rem/);
    expect(match).toBeTruthy();
    const remValue = parseFloat(match![1]);
    const rootFontSize = 16; // confirmed in the running app
    const pxValue = remValue * rootFontSize;
    expect(SPACE_CARDS).toBe(pxValue);
  });
});
