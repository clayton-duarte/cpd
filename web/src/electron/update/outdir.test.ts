import { describe, expect, it } from 'vitest';
import { electronOutDir } from './outdir';

describe('electronOutDir', () => {
  it('electronOutDir defaults to dist-electron', () => {
    expect(electronOutDir({})).toBe('dist-electron');
  });
  it('electronOutDir honours CPD_ELECTRON_OUT', () => {
    expect(electronOutDir({ CPD_ELECTRON_OUT: 'dist-electron.next' })).toBe('dist-electron.next');
  });
  it('electronOutDir rejects absolute paths and parent segments', () => {
    expect(electronOutDir({ CPD_ELECTRON_OUT: 'out_1.x-y' })).toBe('out_1.x-y');
    for (const bad of ['/abs', 'a/b', '.', '..', '../x', '']) {
      expect(() => electronOutDir({ CPD_ELECTRON_OUT: bad }), bad).toThrow(Error);
    }
  });
});
