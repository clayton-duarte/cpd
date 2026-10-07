import { describe, expect, it } from 'vitest';
import { TOKENS, w } from './theme';

describe('theme tokens', () => {
  it('has exactly five keys with the correct hex values', () => {
    expect(Object.keys(TOKENS).sort()).toEqual(
      ['black', 'blue', 'green', 'red', 'white'].sort(),
    );
    expect(TOKENS.black).toBe('#16191d');
    expect(TOKENS.white).toBe('#e6e6e6');
    expect(TOKENS.red).toBe('#e5534b');
    expect(TOKENS.green).toBe('#57ab5a');
    expect(TOKENS.blue).toBe('#539bf5');
  });

  it('w(0.6) returns the expected rgba string', () => {
    expect(w(0.6)).toBe('rgba(230, 230, 230, 0.6)');
  });
});
