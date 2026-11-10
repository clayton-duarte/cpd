import { describe, expect, it } from 'vitest';
import { detectPortHolder, formatPortInUseMessage, type PortHolderOptions } from './port-holder.ts';

describe('port-holder', () => {
  it('names holding process and PID on port in use', async () => {
    // 1. When lsof output is available, it identifies process and PID
    const lsofOutput = [
      'p12345',
      'cnode',
      'n*:3333',
    ].join('\n');

    const lsofOptions: PortHolderOptions = {
      execCommand: async (cmd, args) => {
        if (cmd === 'lsof') {
          return { stdout: lsofOutput, stderr: '', exitCode: 0 };
        }
        return { stdout: '', stderr: 'not found', exitCode: 1 };
      },
    };

    const lsofHolder = await detectPortHolder(3333, lsofOptions);
    expect(lsofHolder).toEqual({ process: 'node', pid: 12345 });

    const lsofMsg = await formatPortInUseMessage(3333, lsofOptions);
    expect(lsofMsg).toContain('3333');
    expect(lsofMsg).toContain('node');
    expect(lsofMsg).toContain('12345');

    // 2. When lsof fails and ss output is available (Linux fallback)
    const ssOutput = 'LISTEN 0 128 0.0.0.0:33333 0.0.0.0:* users:(("vite",pid=54321,fd=4))';

    const ssOptions: PortHolderOptions = {
      execCommand: async (cmd, args) => {
        if (cmd === 'lsof') {
          return { stdout: '', stderr: 'command not found', exitCode: 127 };
        }
        if (cmd === 'ss') {
          return { stdout: ssOutput, stderr: '', exitCode: 0 };
        }
        return { stdout: '', stderr: 'unknown command', exitCode: 1 };
      },
    };

    const ssHolder = await detectPortHolder(33333, ssOptions);
    expect(ssHolder).toEqual({ process: 'vite', pid: 54321 });

    const ssMsg = await formatPortInUseMessage(33333, ssOptions);
    expect(ssMsg).toContain('33333');
    expect(ssMsg).toContain('vite');
    expect(ssMsg).toContain('54321');

    // 3. When neither lsof nor ss identifies the holder, message still reports port and unknown holder
    const emptyOptions: PortHolderOptions = {
      execCommand: async () => ({ stdout: '', stderr: '', exitCode: 1 }),
    };

    const absentHolder = await detectPortHolder(3333, emptyOptions);
    expect(absentHolder).toBeNull();

    const emptyMsg = await formatPortInUseMessage(3333, emptyOptions);
    expect(emptyMsg).toContain('3333');
    expect(emptyMsg).toMatch(/unknown/i);
  });
});
