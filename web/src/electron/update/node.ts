import { spawn, execFile } from 'node:child_process';
import * as fs from 'node:fs';
import type { SpawnFollow } from './follow';
import type { UpdateExec, UpdateFs, UpdateTimers } from './types';

export const nodeExec: UpdateExec = (cmd, args, opts) =>
  new Promise((resolve) => {
    execFile(cmd, args, { cwd: opts.cwd, env: { ...process.env, ...opts.env }, maxBuffer: 64 * 1024 * 1024 }, (error, stdout, stderr) => {
      const code = error === null ? 0 : typeof error.code === 'number' ? error.code : 1;
      resolve({ code, stdout, stderr });
    });
  });

export const nodeFs: UpdateFs = {
  exists: (p) => fs.existsSync(p),
  readText: (p) => (fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null),
  writeText: (p, text) => fs.writeFileSync(p, text),
  rename: (from, to) => fs.renameSync(from, to),
  remove: (p) => fs.rmSync(p, { recursive: true, force: true }),
};

export const nodeTimers: UpdateTimers = {
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export const nodeSpawnFollow: SpawnFollow = (args) => {
  const child = spawn('cpd', args);
  let buf = '';
  return {
    onLine: (cb) => {
      child.stdout.setEncoding('utf8');
      child.stdout.on('data', (chunk: string) => {
        buf += chunk;
        const lines = buf.split('\n');
        buf = lines.pop() ?? '';
        for (const l of lines) if (l !== '') cb(l);
      });
    },
    onExit: (cb) => void child.on('exit', (code) => cb(code)),
    kill: () => void child.kill(),
  };
};
