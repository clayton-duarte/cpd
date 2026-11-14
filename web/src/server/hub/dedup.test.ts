import { expect, it, vi } from 'vitest';
import { createDedupStore, type DedupStorage } from './dedup';

function memory(): DedupStorage & { value: string | undefined } {
  const m = {
    value: undefined as string | undefined,
    read: () => m.value,
    write: (s: string) => {
      m.value = s;
    },
  };
  return m;
}

it('replays the stored result for a repeated key without running exec again', async () => {
  const store = createDedupStore({ storage: memory(), cap: 10 });
  const exec = vi.fn(async () => ({ ok: true, n: 1 }));
  const first = await store.run('k1', exec);
  const second = await store.run('k1', exec);
  expect(exec).toHaveBeenCalledTimes(1);
  expect(second).toEqual(first);
});

it('runs distinct keys independently', async () => {
  const store = createDedupStore({ storage: memory(), cap: 10 });
  const a = vi.fn(async () => 'a');
  const b = vi.fn(async () => 'b');
  expect(await store.run('a', a)).toBe('a');
  expect(await store.run('b', b)).toBe('b');
  expect(a).toHaveBeenCalledTimes(1);
  expect(b).toHaveBeenCalledTimes(1);
  expect(await store.run('a', a)).toBe('a');
  expect(await store.run('b', b)).toBe('b');
  expect(a).toHaveBeenCalledTimes(1);
  expect(b).toHaveBeenCalledTimes(1);
});

it('entries survive a store recreated from the same storage', async () => {
  const storage = memory();
  await createDedupStore({ storage, cap: 10 }).run('k', async () => 'first');
  const exec = vi.fn(async () => 'second');
  const result = await createDedupStore({ storage, cap: 10 }).run('k', exec);
  expect(exec).not.toHaveBeenCalled();
  expect(result).toBe('first');
});

it('evicts the oldest entries beyond the cap', async () => {
  const store = createDedupStore({ storage: memory(), cap: 2 });
  await store.run('a', async () => 'a1');
  await store.run('b', async () => 'b1');
  await store.run('c', async () => 'c1');
  const a = vi.fn(async () => 'a2');
  const b = vi.fn(async () => 'b2');
  const c = vi.fn(async () => 'c2');
  expect(await store.run('a', a)).toBe('a2');
  expect(a).toHaveBeenCalledTimes(1);
  expect(await store.run('c', c)).toBe('c1');
  expect(c).not.toHaveBeenCalled();
  expect(b).not.toHaveBeenCalled();
});

it('a failing exec is stored as its error result and not re-run', async () => {
  const store = createDedupStore({ storage: memory(), cap: 10 });
  const exec = vi.fn(async () => {
    throw new Error('boom');
  });
  await expect(store.run('k', exec)).rejects.toThrow('boom');
  await expect(store.run('k', exec)).rejects.toThrow('boom');
  expect(exec).toHaveBeenCalledTimes(1);
});
