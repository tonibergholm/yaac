// Verifies the built sidebar against the packaging rules: the strict CSP is present verbatim,
// every asset reference is relative, inline scripts and inline styles are absent, and no
// external URL appears in the output. `pnpm build` runs it after `vite build`.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SIDEBAR_CSP } from '../csp.mjs';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));
const problems = [];

// Namespace identifiers in HTML and CSS are not requests. The CSP enforces the rule at runtime,
// through `connect-src app://ui`. This script catches the obvious cases in the build output.
const ALLOWED_URLS = new Set(['http://www.w3.org/2000/svg', 'http://www.w3.org/1999/xhtml']);
// Literal URLs in JS are mostly namespaces and error messages. Only network APIs are checked.
const JS_NETWORK = [
  /\b(XMLHttpRequest|WebSocket|EventSource|sendBeacon|importScripts)\b/,
  /\bfetch\(\s*["'`](https?:|\/\/)/,
  /\bimport\(\s*["'`]https?:/,
];

function walk(dir) {
  const files = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) files.push(...walk(path));
    else files.push(path);
  }
  return files;
}

const files = walk(dist);
const html = readFileSync(join(dist, 'sidebar.html'), 'utf8');

const cspMeta = `<meta http-equiv="Content-Security-Policy" content="${SIDEBAR_CSP}" />`;
if (!html.includes(cspMeta)) problems.push('sidebar.html: strict CSP meta tag missing or altered');

for (const [, value] of html.matchAll(/\s(?:src|href)="([^"]*)"/g)) {
  if (!value.startsWith('./')) problems.push(`sidebar.html: non-relative asset reference "${value}"`);
}

for (const [, body] of html.matchAll(/<script(?![^>]*\ssrc=)[^>]*>([\s\S]*?)<\/script>/g)) {
  if (body.trim() !== '') problems.push('sidebar.html: inline <script> with content');
}
if (/\sstyle="/.test(html)) problems.push('sidebar.html: inline style attribute');

for (const file of files) {
  const name = relative(dist, file);
  if (!/\.(html|css|js)$/.test(name)) continue;
  const text = readFileSync(file, 'utf8');
  if (name.endsWith('.js')) {
    for (const pattern of JS_NETWORK) {
      if (pattern.test(text)) problems.push(`${name}: network API or external import matches ${pattern}`);
    }
  } else {
    for (const [url] of text.matchAll(/https?:\/\/[^\s"'`)<>\\]+/g)) {
      if (!ALLOWED_URLS.has(url)) problems.push(`${name}: external URL ${url}`);
    }
  }
  if (name.endsWith('.css') && /url\(\s*['"]?(https?:|\/\/)/.test(text)) {
    problems.push(`${name}: external url() in stylesheet`);
  }
}

if (problems.length > 0) {
  console.error(`check-dist: ${problems.length} problem(s)`);
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}
console.log(`check-dist: ok (${files.length} files, CSP verified, no external references)`);
