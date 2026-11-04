// Boundary types for the parts of @aurigax-ai/ghostty-web that renderer.ts uses.
// The package ships TypeScript source that does not pass this repo's strict tsconfig,
// so tsconfig `paths` maps the import to this file for the typechecker (the bundler still uses the real package).
export interface ITheme {
  [key: string]: string;
}
export class Ghostty {
  static fromBytes(bytes: Uint8Array): Promise<Ghostty>;
}
export interface ITerminalOptions {
  ghostty?: Ghostty;
  renderer?: "webgl" | "canvas";
  theme?: ITheme;
  fontFamily?: string;
  fontSize?: number;
}
export interface ITerminalAddon {
  activate(terminal: Terminal): void;
  dispose(): void;
}
export class Terminal {
  constructor(options?: ITerminalOptions);
  readonly cols: number;
  readonly rows: number;
  clipboardWriteHandler: ((text: string) => boolean) | null;
  open(parent: HTMLElement): void;
  write(data: string | Uint8Array): void;
  focus(): void;
  dispose(): void;
  loadAddon(addon: ITerminalAddon): void;
  setPaused(paused: boolean): void;
  resize(cols: number, rows: number): void;
  onData(listener: (data: string) => void): { dispose(): void };
  onResize(listener: (size: { cols: number; rows: number }) => void): { dispose(): void };
}
export class FitAddon implements ITerminalAddon {
  activate(terminal: Terminal): void;
  dispose(): void;
  fit(): void;
  proposeDimensions(): { cols: number; rows: number } | undefined;
}
