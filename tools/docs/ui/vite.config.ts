import { spawn } from 'node:child_process';
import { watchFile } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join, normalize, resolve, sep } from 'node:path';
import { defineConfig, normalizePath, type Plugin } from 'vite';

const TEXT_TYPES: Record<string, string> = { '.md': 'text/markdown', '.txt': 'text/plain' };

/**
 * Serves Markdown and text files with `charset=utf-8`. Without it browsers read them as
 * Latin-1 and garble Russian and Hindi. A production web server needs the same setting.
 */
function utf8TextFiles(): Plugin {
  const serveFrom = (root: string) => async (
    req: { url?: string },
    res: { setHeader(name: string, value: string): void; end(body: Buffer): void },
    next: () => void,
  ) => {
    const path = decodeURIComponent((req.url ?? '').split('?')[0]);
    const extension = Object.keys(TEXT_TYPES).find((ext) => path.endsWith(ext));
    const file = normalize(join(root, path));
    if (!extension || !file.startsWith(root + sep)) return next();
    try {
      const body = await readFile(file);
      res.setHeader('Content-Type', `${TEXT_TYPES[extension]}; charset=utf-8`);
      res.end(body);
    } catch {
      next();
    }
  };
  return {
    name: 'utf8-text-files',
    configureServer(server) {
      server.middlewares.use(serveFrom(normalize(server.config.publicDir)));
    },
    configurePreviewServer(server) {
      server.middlewares.use(serveFrom(resolve(server.config.root, server.config.build.outDir)));
    },
  };
}

/**
 * Live docs in development: when anything in `docs/` (or the engine's code) changes,
 * rebuilds the site data and tells the open pages to reload it. If the docs do not pass
 * the checks, the errors show up in the browser instead.
 */
function liveDocs(): Plugin {
  return {
    name: 'live-docs',
    apply: 'serve',
    configureServer(server) {
      const repo = resolve(server.config.root, '../../..');
      const watched = [join(repo, 'docs'), join(repo, 'tools', 'docs', 'engine', 'src')];
      const prefixes = watched.map((dir) => `${normalizePath(dir)}/`);
      server.watcher.add(watched);

      let running = false;
      let pending = false;
      let timer: ReturnType<typeof setTimeout> | undefined;

      const fail = (message: string) => {
        server.config.logger.error(message, { timestamp: true });
        server.ws.send({ type: 'error', err: { message, stack: '', plugin: 'live-docs' } });
      };

      const rebuild = () => {
        if (running) {
          pending = true;
          return;
        }
        running = true;
        const started = Date.now();
        let output = '';
        const engine = spawn('cargo', ['run', '--quiet', '-p', 'docs-engine', '--', 'build'], { cwd: repo });
        engine.stdout.on('data', (chunk) => (output += chunk));
        engine.stderr.on('data', (chunk) => (output += chunk));
        engine.on('error', (error) => fail(`Could not start the docs engine: ${error.message}`));
        engine.on('close', (code) => {
          running = false;
          if (code === 0) {
            server.config.logger.info(`docs rebuilt in ${Date.now() - started} ms`, { timestamp: true });
            server.ws.send({ type: 'custom', event: 'docs:updated' });
          } else if (code !== null) {
            fail(output.trim());
          }
          if (pending) {
            pending = false;
            rebuild();
          }
        });
      };

      // Editors often write a file in several steps, so wait until the changes settle.
      const schedule = () => {
        clearTimeout(timer);
        timer = setTimeout(rebuild, 200);
      };
      const onChange = (file: string) => {
        if (prefixes.some((prefix) => normalizePath(file).startsWith(prefix))) schedule();
      };
      server.watcher.on('add', onChange).on('change', onChange).on('unlink', onChange);

      // Vite's watcher skips .git, so Git's log of HEAD and the tags are checked on their own:
      // a new commit or tag then shows up in the page history and the versions at once.
      const git = join(repo, '.git');
      for (const file of [join(git, 'logs', 'HEAD'), join(git, 'refs', 'tags'), join(git, 'packed-refs')]) {
        watchFile(file, { interval: 1000 }, (current, previous) => {
          if (current.mtimeMs !== previous.mtimeMs) schedule();
        });
      }
    },
  };
}

export default defineConfig({
  plugins: [utf8TextFiles(), liveDocs()],
  // The app loads its data with top-level await.
  build: { target: 'es2022' },
  server: {
    port: 5176,
    strictPort: true,
    // In a container the files come from the host, and changes to them don't arrive as
    // events, so the container sets DOCS_POLL and the files are polled instead.
    watch: process.env.DOCS_POLL ? { usePolling: true, interval: 1000 } : undefined,
  },
});
