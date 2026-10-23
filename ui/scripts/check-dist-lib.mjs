// Packaging rules for the built sidebar, as a pure function over file contents so tests can feed it
// bad output. scripts/check-dist.mjs reads the real dist/ files and calls this.
import { SIDEBAR_CSP } from '../csp.mjs';

// Namespace identifiers in HTML and CSS are not requests. The CSP enforces the rule at runtime,
// through `connect-src app://ui`. This check catches the obvious cases in the build output.
const ALLOWED_URLS = new Set(['http://www.w3.org/2000/svg', 'http://www.w3.org/1999/xhtml']);

// Literal URLs in JS are mostly namespaces and error messages. Only network APIs are checked.
const JS_NETWORK = [
  /\b(XMLHttpRequest|WebSocket|EventSource|sendBeacon|importScripts)\b/,
  /\bfetch\(\s*["'`](https?:|\/\/)/,
  /\bimport\(\s*["'`]https?:/,
];

/**
 * Returns a list of problems. An empty list means the output passes.
 * @param {Record<string, string>} files relative path to file contents
 * @returns {string[]}
 */
export function checkDistFiles(files) {
  const problems = [];
  const html = files['sidebar.html'];
  if (html === undefined) return ['sidebar.html: missing'];

  // The strict CSP must be present verbatim. The host also sends it as a response header.
  const cspMeta = `<meta http-equiv="Content-Security-Policy" content="${SIDEBAR_CSP}" />`;
  if (!html.includes(cspMeta)) problems.push('sidebar.html: strict CSP meta tag missing or altered');

  for (const [, value] of html.matchAll(/\s(?:src|href)="([^"]*)"/g)) {
    if (!value.startsWith('./')) problems.push(`sidebar.html: non-relative asset reference "${value}"`);
  }

  // Any script without src is inline, even an empty one. The CSP has no 'unsafe-inline'.
  for (const [, attributes] of html.matchAll(/<script\b([^>]*)>/gi)) {
    if (!/\ssrc\s*=/i.test(attributes ?? '')) problems.push('sidebar.html: inline <script> element');
  }
  if (/<style\b/i.test(html)) problems.push('sidebar.html: inline <style> element');
  if (/\sstyle\s*=/i.test(html)) problems.push('sidebar.html: inline style attribute');
  if (/<[a-z][^>]*\son[a-z]+\s*=/i.test(html)) problems.push('sidebar.html: inline event handler attribute');

  for (const [name, text] of Object.entries(files)) {
    if (!/\.(html|css|js)$/.test(name)) continue;
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
  return problems;
}
