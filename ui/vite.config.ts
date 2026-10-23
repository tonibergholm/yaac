/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from 'vite';
import { SIDEBAR_CSP } from './csp.mjs';

// The CSP is injected into the built HTML only: the dev server needs inline scripts and a
// websocket for HMR, which this policy blocks. `scripts/check-dist.mjs` verifies the output.
function strictCsp(): Plugin {
  return {
    name: 'yaac-strict-csp',
    apply: 'build',
    transformIndexHtml: {
      order: 'pre',
      handler(html) {
        return html.replace(
          /<head>/i,
          `<head>\n    <meta http-equiv="Content-Security-Policy" content="${SIDEBAR_CSP}" />`,
        );
      },
    },
  };
}

export default defineConfig({
  base: './',
  plugins: [strictCsp()],
  oxc: {
    jsx: { runtime: 'automatic', importSource: 'preact' },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
    rollupOptions: {
      input: 'sidebar.html',
    },
  },
  server: {
    host: '127.0.0.1',
  },
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.{ts,tsx}'],
    restoreMocks: true,
  },
});
