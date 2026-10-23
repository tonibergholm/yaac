import { describe, expect, it } from 'vitest';
import { SIDEBAR_CSP } from '../csp.mjs';
import { checkDistFiles } from '../scripts/check-dist-lib.mjs';

const CSP_META = `<meta http-equiv="Content-Security-Policy" content="${SIDEBAR_CSP}" />`;

function page(head: string, body = '<div id="app"></div>'): string {
  return `<!doctype html>\n<html lang="en">\n  <head>\n    ${CSP_META}\n    ${head}\n  </head>\n  <body>${body}</body>\n</html>\n`;
}

const GOOD_HEAD = '<script type="module" crossorigin src="./assets/sidebar.js"></script>\n    <link rel="stylesheet" crossorigin href="./assets/sidebar.css">';

describe('check-dist', () => {
  it('passes a page with the CSP, external scripts and styles, and relative references', () => {
    const problems = checkDistFiles({
      'sidebar.html': page(GOOD_HEAD),
      'assets/sidebar.js': 'export const x = 1;',
      'assets/sidebar.css': 'body { color: black; }',
    });
    expect(problems).toStrictEqual([]);
  });

  it('fails when the CSP is missing or changed', () => {
    const problems = checkDistFiles({ 'sidebar.html': '<!doctype html><html><head></head></html>' });
    expect(problems.join('\n')).toMatch(/CSP/);
  });

  it.each([
    ['an empty inline script', '<script></script>'],
    ['an inline script with code', '<script>window.x = 1</script>'],
    ['an inline module script', '<script type="module">import "./a.js"</script>'],
  ])('fails on %s', (_name, head) => {
    const problems = checkDistFiles({ 'sidebar.html': page(GOOD_HEAD + `\n${head}`) });
    expect(problems.some((p) => /inline <script>/.test(p))).toBe(true);
  });

  it('fails on an inline style element', () => {
    const problems = checkDistFiles({ 'sidebar.html': page(GOOD_HEAD + '\n<style>body { color: red }</style>') });
    expect(problems.some((p) => /inline <style>/.test(p))).toBe(true);
  });

  it('fails on a style attribute', () => {
    const problems = checkDistFiles({ 'sidebar.html': page(GOOD_HEAD, '<div id="app" style="color: red"></div>') });
    expect(problems.some((p) => /style attribute/.test(p))).toBe(true);
  });

  it('fails on an inline event handler', () => {
    const problems = checkDistFiles({ 'sidebar.html': page(GOOD_HEAD, '<button onclick="x()">x</button>') });
    expect(problems.some((p) => /event handler/.test(p))).toBe(true);
  });

  it('fails on an absolute asset reference', () => {
    const problems = checkDistFiles({ 'sidebar.html': page('<script type="module" src="/assets/a.js"></script>') });
    expect(problems.some((p) => /non-relative/.test(p))).toBe(true);
  });

  it('fails on an external URL in a stylesheet', () => {
    const problems = checkDistFiles({
      'sidebar.html': page(GOOD_HEAD),
      'assets/sidebar.css': '@font-face { src: url(https://fonts.example/x.woff2); }',
    });
    expect(problems.some((p) => /url\(\)/.test(p))).toBe(true);
  });

  it('fails on a network API in the script', () => {
    const problems = checkDistFiles({
      'sidebar.html': page(GOOD_HEAD),
      'assets/sidebar.js': 'fetch("https://analytics.example/x")',
    });
    expect(problems.some((p) => /network API/.test(p))).toBe(true);
  });

  it('fails when sidebar.html is missing', () => {
    expect(checkDistFiles({})).toStrictEqual(['sidebar.html: missing']);
  });
});
