// Content-Security-Policy for the sidebar page. vite.config.ts injects it into the built HTML only.
// scripts/check-dist.mjs verifies that the built page carries it verbatim.
export const SIDEBAR_CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self' data:",
  'connect-src app://ui',
  "frame-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');
