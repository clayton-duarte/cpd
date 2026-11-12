/**
 * Production WebUI server serving built renderer static files and /cpd-ws on 0.0.0.0:3333.
 * With port-in-use fail-loud holder detection (pro-d541, pro-d542, web/CONTRACT.md §15.1).
 */

import * as http from 'node:http';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer as createViteServer, type ViteDevServer } from 'vite';
import { formatPortInUseMessage } from './port-holder.ts';
import { createServices, realBoundaries, cpdWebPlugin, type PluginBoundaries } from './plugin.ts';
import { createWsServer, createUpgradeFactory, WS_PATH, type WebSocketUpgradeServer } from './ws.ts';
import { writeWebuiToken } from './webui-token.ts';

export const PROD_PORT = 3333;
export const PROD_HOST = '0.0.0.0';

export const PREVIEW_PORT = 33333;
export const PREVIEW_HOST = '0.0.0.0';

const MIME_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.mjs': 'application/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
};

export interface ServeOptions {
  port?: number;
  host?: string;
  staticDir?: string;
  boundaries?: PluginBoundaries;
}

export function findStaticDir(): string {
  const here = path.dirname(fileURLToPath(import.meta.url));
  // In dist-electron/server.js: here is dist-electron, browser dist is in ../dist or dist
  const distCandidate = path.resolve(here, '..', 'dist');
  if (fs.existsSync(distCandidate)) return distCandidate;

  // In source tree: web/src/server -> web/dist
  const webDistCandidate = path.resolve(here, '..', '..', 'dist');
  if (fs.existsSync(webDistCandidate)) return webDistCandidate;

  return distCandidate;
}

export async function createProdServer(options?: ServeOptions): Promise<http.Server> {
  const port = options?.port ?? PROD_PORT;
  const host = options?.host ?? PROD_HOST;
  const staticDir = options?.staticDir ?? findStaticDir();

  const boundaries = options?.boundaries ?? ((): PluginBoundaries => {
    const real = realBoundaries(process.env);
    const issued = writeWebuiToken(real.cpdHome);
    return { ...real, webuiToken: issued.token };
  })();

  const services = createServices(boundaries);
  const started = services.cockpit.start();
  started.catch((e: unknown) => {
    console.error('cpd-web: cockpit failed to start:', e instanceof Error ? e.message : String(e));
  });

  const server = http.createServer((req, res) => {
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      res.writeHead(405, { 'Content-Type': 'text/plain' });
      res.end('Method Not Allowed');
      return;
    }

    const urlPath = decodeURIComponent(new URL(req.url ?? '/', `http://${req.headers.host ?? 'localhost'}`).pathname);
    const relative = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
    const filePath = path.normalize(path.join(staticDir, relative));

    // Guard against path traversal
    if (!filePath.startsWith(staticDir)) {
      res.writeHead(403, { 'Content-Type': 'text/plain' });
      res.end('Forbidden');
      return;
    }

    fs.stat(filePath, (err, stats) => {
      if (err || !stats.isFile()) {
        // Fallback to index.html for client-side routing if applicable
        const indexPath = path.join(staticDir, 'index.html');
        fs.stat(indexPath, (indexErr, indexStats) => {
          if (!indexErr && indexStats.isFile()) {
            res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
            if (req.method === 'HEAD') {
              res.end();
            } else {
              fs.createReadStream(indexPath).pipe(res);
            }
          } else {
            res.writeHead(404, { 'Content-Type': 'text/plain' });
            res.end('Not Found');
          }
        });
        return;
      }

      const ext = path.extname(filePath).toLowerCase();
      const contentType = MIME_TYPES[ext] ?? 'application/octet-stream';
      res.writeHead(200, {
        'Content-Type': contentType,
        'Content-Length': stats.size,
      });

      if (req.method === 'HEAD') {
        res.end();
      } else {
        fs.createReadStream(filePath).pipe(res);
      }
    });
  });

  createWsServer({
    newConnection: (send) => services.newConnection(send),
    server: createUpgradeFactory(server as unknown as WebSocketUpgradeServer),
    scheduler: boundaries.scheduler,
    started: () => started,
    onError: (e) => console.error('cpd-ws error:', e),
    path: WS_PATH,
  });

  return new Promise((resolve, reject) => {
    server.once('error', async (err: NodeJS.ErrnoException) => {
      if (err.code === 'EADDRINUSE') {
        const msg = await formatPortInUseMessage(port);
        console.error(msg);
        process.exit(1);
      } else {
        reject(err);
      }
    });

    server.listen(port, host, () => {
      resolve(server);
    });
  });
}

export interface PreviewServeOptions {
  port?: number;
  host?: string;
  boundaries?: PluginBoundaries;
}

export async function createPreviewServer(options?: PreviewServeOptions): Promise<ViteDevServer> {
  const port = options?.port ?? PREVIEW_PORT;
  const host = options?.host ?? PREVIEW_HOST;
  const boundaries = options?.boundaries;

  const server = await createViteServer({
    configFile: false,
    plugins: [cpdWebPlugin(boundaries ? { boundaries } : undefined)],
    server: {
      host,
      port,
      strictPort: true,
    },
  });

  return server;
}

// If run directly as a script
if (import.meta.url.startsWith('file:') && process.argv[1] === fileURLToPath(import.meta.url)) {
  createProdServer().catch((err) => {
    console.error('Failed to start production WebUI server:', err);
    process.exit(1);
  });
}
