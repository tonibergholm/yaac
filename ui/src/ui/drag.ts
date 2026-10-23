/**
 * Random token for this page session, from 16 bytes of `crypto.getRandomValues`. It lives in module
 * memory only and is never written to storage, the URL or the DOM. A page in a content view cannot
 * read it, so it cannot start a drag this sidebar accepts as internal.
 */
function sessionToken(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/** MIME type of every tree drag in this session. Only drags that carry exactly this type are internal. */
export const NODE_MIME = `application/x-yaac-node-${sessionToken()}`;

/**
 * True when the drag started inside this sidebar session. The check is an exact match on the type
 * list, so the plain `application/x-yaac-node` type, or any other type, is foreign.
 */
export function isInternalDrag(transfer: DataTransfer | null | undefined): boolean {
  if (!transfer) return false;
  return Array.from(transfer.types).includes(NODE_MIME);
}
