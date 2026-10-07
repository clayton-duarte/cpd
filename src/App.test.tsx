import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * jsdom does not perform real layout, so a rendered box-measurement assertion
 * would not be meaningful here (it would pass regardless of whether the bar
 * actually clips). Per the B5 card, the honest alternative is a static guard:
 * fail if the top bar's `Group` is ever given a fixed `h` prop again, which is
 * exactly the defect this card fixes (a content-sized two-line bar snapped to
 * a smaller fixed/token height, clipping its second line).
 */
describe('TopBar does not clip its two-line content', () => {
  it('does not set a fixed height on the top bar Group', () => {
    const appPath = join(process.cwd(), 'src', 'App.tsx');
    const appSource = readFileSync(appPath, 'utf-8');
    const topBarMatch = appSource.match(/function TopBar\(\)[\s\S]*?\n}\n/);
    expect(topBarMatch, 'TopBar function not found in App.tsx').toBeTruthy();
    const topBarSource = topBarMatch![0];

    // The bar must size itself from its content (session name + project line).
    // A fixed `h` prop (number or token) reintroduces the clipping defect.
    expect(topBarSource).not.toMatch(/\bh=\{/);
    expect(topBarSource).not.toMatch(/\bh="/);

    // It should instead derive its height from vertical padding on the scale.
    expect(topBarSource).toMatch(/py="var\(--space-\d+\)"/);
  });
});
