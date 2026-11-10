import { describe, expect, it } from 'vitest';
import { detectPortHolder } from './port-holder.ts';

describe('port-holder', () => {
  it('names holding process and PID on port in use', async () => {
    // Stub returns null, so this assertion will fail
    const holder = await detectPortHolder(3333);
    expect(holder).not.toBeNull();
    expect(holder?.process).toBe('node');
    expect(holder?.pid).toBe(12345);
  });
});
