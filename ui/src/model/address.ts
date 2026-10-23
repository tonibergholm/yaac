export type AddressResult = { ok: true; url: string } | { ok: false; error: string };

/** Cap on the final URL, after percent-encoding. */
export const MAX_ADDRESS_LENGTH = 2048;

const SCHEME = /^([a-z][a-z0-9+.-]*):/i;
/** `host:port` with an optional path, query or fragment. Whitespace is rejected before this runs. */
const HOST_PORT = /^([a-z0-9.-]+):(\d{1,5})(?:[/?#].*)?$/i;

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

/** The host part of an address without a scheme: a bracketed IPv6 literal, or the text before any port or path. */
function hostOf(raw: string): string {
  const bracketed = /^\[[^\]]*\]/.exec(raw);
  if (bracketed) return bracketed[0];
  return (raw.split(/[/?#]/)[0] ?? raw).replace(/:\d*$/, '');
}

/** Parses an IPv6 literal (without brackets) into eight 16-bit groups, or null when it is not one. */
function parseIpv6(text: string): number[] | null {
  let s = text;
  const embedded = /^(.*:)(\d{1,3}(?:\.\d{1,3}){3})$/.exec(s);
  if (embedded) {
    const parts = (embedded[2] ?? '').split('.').map(Number);
    if (parts.some((part) => part > 255)) return null;
    const high = ((parts[0] ?? 0) << 8) | (parts[1] ?? 0);
    const low = ((parts[2] ?? 0) << 8) | (parts[3] ?? 0);
    s = `${embedded[1] ?? ''}${high.toString(16)}:${low.toString(16)}`;
  }
  const halves = s.split('::');
  if (halves.length > 2) return null;
  const head = halves[0] ? halves[0].split(':') : [];
  const tail = halves.length === 2 && halves[1] ? halves[1].split(':') : [];
  let groups: string[];
  if (halves.length === 2) {
    const fill = 8 - head.length - tail.length;
    if (fill < 1) return null;
    groups = [...head, ...Array<string>(fill).fill('0'), ...tail];
  } else {
    groups = head;
  }
  if (groups.length !== 8) return null;
  const numbers: number[] = [];
  for (const group of groups) {
    if (!/^[0-9a-f]{1,4}$/i.test(group)) return null;
    numbers.push(parseInt(group, 16));
  }
  return numbers;
}

/** `::1` in any spelling, or an IPv4-mapped 127.x.x.x address. */
function isLoopbackIpv6(text: string): boolean {
  const n = parseIpv6(text);
  if (!n) return false;
  if (n.slice(0, 7).every((group) => group === 0) && n[7] === 1) return true;
  return n.slice(0, 5).every((group) => group === 0) && n[5] === 0xffff && (n[6] ?? 0) >> 8 === 127;
}

function isLoopbackHost(host: string): boolean {
  const lower = host.toLowerCase().replace(/\.$/, '');
  if (lower === 'localhost' || lower.endsWith('.localhost')) return true;
  if (lower.startsWith('[') && lower.endsWith(']')) return isLoopbackIpv6(lower.slice(1, -1));
  return /^127(?:\.\d{1,3}){3}$/.test(lower);
}

/**
 * Scheme for an address typed without one. IPv6 literals and loopback hosts (in any form) default
 * to http. Every other host defaults to https.
 */
function defaultScheme(host: string): 'http://' | 'https://' {
  return host.startsWith('[') || isLoopbackHost(host) ? 'http://' : 'https://';
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
  // The cap applies to the URL the browser will load, after percent-encoding.
  if (parsed.href.length > MAX_ADDRESS_LENGTH) {
    return fail(`Addresses are limited to ${MAX_ADDRESS_LENGTH} characters after encoding.`);
  }
  return { ok: true, url: parsed.href };
}

/**
 * Turns what the user typed into a navigable URL. Only http, https and `about:blank` pass.
 * Search is not available in M1, so text that is not an address is rejected.
 */
export function normalizeAddress(input: string): AddressResult {
  const raw = input.trim();
  if (raw === '') return fail('Enter an address.');
  if (hasControlOrSpace(raw)) {
    return fail('Addresses cannot contain spaces. Search is not available yet.');
  }

  const hostPort = HOST_PORT.exec(raw);
  if (hostPort && !KNOWN_SCHEMES.has((hostPort[1] ?? '').toLowerCase())) {
    return parseWeb(`${defaultScheme(hostPort[1] ?? '')}${raw}`);
  }

  const scheme = SCHEME.exec(raw)?.[1]?.toLowerCase();
  if (scheme !== undefined) {
    if (scheme === 'about') {
      return raw.toLowerCase() === 'about:blank' ? { ok: true, url: 'about:blank' } : fail(ALLOWED_MESSAGE);
    }
    if (scheme !== 'http' && scheme !== 'https') return fail(ALLOWED_MESSAGE);
    return parseWeb(raw);
  }

  return parseWeb(`${defaultScheme(hostOf(raw))}${raw}`);
}
