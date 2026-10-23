export type AddressResult = { ok: true; url: string } | { ok: false; error: string };

export const MAX_ADDRESS_LENGTH = 2048;

const SCHEME = /^([a-z][a-z0-9+.-]*):/i;
/** `host:port` with an optional path, query or fragment. Whitespace is rejected before this runs. */
const HOST_PORT = /^([a-z0-9.-]+):(\d{1,5})(?:[/?#].*)?$/i;
const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/;

/** Schemes that can appear before a colon. A `word:number` input with one of these is a scheme, not a host. */
const KNOWN_SCHEMES = new Set([
  'about',
  'app',
  'blob',
  'chrome',
  'content',
  'data',
  'file',
  'ftp',
  'http',
  'https',
  'intent',
  'javascript',
  'mailto',
  'sms',
  'tel',
  'vbscript',
  'view-source',
  'ws',
  'wss',
]);

const ALLOWED_MESSAGE = 'Only http://, https:// and about:blank addresses are allowed.';

function fail(error: string): AddressResult {
  return { ok: false, error };
}

/**
 * ASCII control characters and spaces. The URL parser strips some of these inside a scheme,
 * as in `java\tscript:`, so they are rejected before parsing.
 */
function hasControlOrSpace(text: string): boolean {
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code <= 0x20 || code === 0x7f) return true;
  }
  return false;
}

/** Local hosts and IPv4 addresses default to http. Everything else defaults to https. */
function defaultScheme(host: string): 'http://' | 'https://' {
  const lower = host.toLowerCase();
  return lower === 'localhost' || IPV4.test(lower) ? 'http://' : 'https://';
}

function parseWeb(candidate: string): AddressResult {
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return fail('That is not a valid address. Include the full address, such as https://example.com.');
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return fail(ALLOWED_MESSAGE);
  if (parsed.hostname === '') return fail('That address has no host name.');
  return { ok: true, url: parsed.href };
}

/**
 * Turns what the user typed into a navigable URL. Only http, https and `about:blank` pass.
 * Search is not available in M1, so text that is not an address is rejected.
 */
export function normalizeAddress(input: string): AddressResult {
  const raw = input.trim();
  if (raw === '') return fail('Enter an address.');
  if (raw.length > MAX_ADDRESS_LENGTH) {
    return fail(`Addresses are limited to ${MAX_ADDRESS_LENGTH} characters.`);
  }
  if (hasControlOrSpace(raw)) {
    return fail('Addresses cannot contain spaces. Search is not available yet.');
  }

  const hostPort = HOST_PORT.exec(raw);
  if (hostPort && !KNOWN_SCHEMES.has((hostPort[1] ?? '').toLowerCase())) {
    const host = hostPort[1] ?? '';
    return parseWeb(`${defaultScheme(host)}${raw}`);
  }

  const scheme = SCHEME.exec(raw)?.[1]?.toLowerCase();
  if (scheme !== undefined) {
    if (scheme === 'about') {
      return raw.toLowerCase() === 'about:blank' ? { ok: true, url: 'about:blank' } : fail(ALLOWED_MESSAGE);
    }
    if (scheme !== 'http' && scheme !== 'https') return fail(ALLOWED_MESSAGE);
    return parseWeb(raw);
  }

  const hostOnly = raw.split(/[/?#]/)[0] ?? raw;
  return parseWeb(`${defaultScheme(hostOnly)}${raw}`);
}
