import { describe, expect, it } from 'vitest';

// L7b: jsdom 30.1.2 does not provide window.localStorage natively (see the
// stub installed in test-setup.ts). This asserts the stub itself behaves
// like real Storage, so a future jsdom upgrade that restores native
// localStorage does not silently change behaviour underneath the app.
describe('localStorage stub (jsdom 30 workaround)', () => {
  it('is defined on both the bare global and window', () => {
    expect(typeof localStorage).not.toBe('undefined');
    expect(window.localStorage).toBeDefined();
    expect(window.localStorage).toBe(localStorage);
  });

  it('supports set, get, and clear', () => {
    localStorage.setItem('probe-key', 'probe-value');
    expect(localStorage.getItem('probe-key')).toBe('probe-value');

    localStorage.clear();
    expect(localStorage.getItem('probe-key')).toBeNull();
    expect(localStorage.length).toBe(0);
  });

  it('removeItem deletes a single key without clearing others', () => {
    localStorage.setItem('a', '1');
    localStorage.setItem('b', '2');
    localStorage.removeItem('a');

    expect(localStorage.getItem('a')).toBeNull();
    expect(localStorage.getItem('b')).toBe('2');

    localStorage.clear();
  });
});
