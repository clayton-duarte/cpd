// @vitest-environment node
import { expect, it } from 'vitest';
import { readPreviewMode } from './preview-mode';

it('readPreviewMode defaults to production when CPD_MODE is unset', () => {
  expect(readPreviewMode({})).toEqual({ mode: 'production' });
  expect(readPreviewMode({ CPD_MODE: '' })).toEqual({ mode: 'production' });
});

it('readPreviewMode ignores the dev URL in production', () => {
  const url = { CPD_ELECTRON_DEV_URL: 'http://localhost:5173/electron.html', CPD_CHECKOUT: '/w' };
  expect(readPreviewMode(url)).toEqual({ mode: 'production' });
  expect(readPreviewMode({ ...url, CPD_MODE: 'production' })).toEqual({ mode: 'production' });
});

it('readPreviewMode returns the checkout and dev URL in preview', () => {
  expect(readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '/w', CPD_ELECTRON_DEV_URL: 'http://localhost:5173/electron.html' })).toEqual({
    mode: 'preview',
    checkout: '/w',
    devUrl: 'http://localhost:5173/electron.html',
  });
  expect(readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '/w', CPD_ELECTRON_DEV_URL: '' })).toEqual({
    mode: 'preview',
    checkout: '/w',
    devUrl: 'http://localhost:5173/electron.html',
  });
});

it('readPreviewMode rejects an unknown CPD_MODE', () => {
  expect(() => readPreviewMode({ CPD_MODE: 'staging' })).toThrow('CPD_MODE must be production or preview, got "staging"');
});

it('an explicit production request beats a preview environment', () => {
  // The relaunch carries the request as an argument (--cpd-mode=production): Electron's
  // relaunch keeps the original environment, so the argument must beat CPD_MODE=preview.
  const env = { CPD_MODE: 'preview', CPD_CHECKOUT: '/w', CPD_ELECTRON_DEV_URL: 'http://127.0.0.1:5173/electron.html' };
  const read = readPreviewMode as unknown as (
    env: Readonly<Record<string, string | undefined>>,
    args: readonly string[],
  ) => ReturnType<typeof readPreviewMode>;
  const mode = read(env, ['--cpd-mode=production']);
  expect(mode).toEqual({ mode: 'production' });
  expect('devUrl' in mode).toBe(false);
});

it('readPreviewMode rejects preview without CPD_CHECKOUT', () => {
  expect(() => readPreviewMode({ CPD_MODE: 'preview' })).toThrow('CPD_MODE=preview needs CPD_CHECKOUT');
  expect(() => readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '' })).toThrow('CPD_MODE=preview needs CPD_CHECKOUT');
});

it('resolves the dev URL from CPD_ELECTRON_DEV_URL with the electron.html default', () => {
  expect(readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '/w' })).toEqual({
    mode: 'preview',
    checkout: '/w',
    devUrl: 'http://localhost:5173/electron.html',
  });
  expect(readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '/w', CPD_ELECTRON_DEV_URL: '' })).toEqual({
    mode: 'preview',
    checkout: '/w',
    devUrl: 'http://localhost:5173/electron.html',
  });
  expect(readPreviewMode({ CPD_MODE: 'preview', CPD_CHECKOUT: '/w', CPD_ELECTRON_DEV_URL: 'http://127.0.0.1:5173/electron.html' })).toEqual({
    mode: 'preview',
    checkout: '/w',
    devUrl: 'http://127.0.0.1:5173/electron.html',
  });
});
