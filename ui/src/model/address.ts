export type AddressResult = { ok: true; url: string } | { ok: false; error: string };

const SCHEME = /^([a-z][a-z0-9+.-]*):/i;

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

  const scheme = SCHEME.exec(raw)?.[1]?.toLowerCase();
  if (scheme !== undefined) {
    if (scheme === 'about') {
      return raw.toLowerCase() === 'about:blank' ? { ok: true, url: 'about:blank' } : fail(ALLOWED_MESSAGE);
    }
    if (scheme !== 'http' && scheme !== 'https') return fail(ALLOWED_MESSAGE);
  }

  const candidate = scheme === undefined ? `https://${raw}` : raw;
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
