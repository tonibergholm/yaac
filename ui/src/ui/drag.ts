/** Custom MIME type set on every tree drag. A drag that carries it started inside the sidebar. */
export const NODE_MIME = 'application/x-yaac-node';

/**
 * True when the drag started inside the sidebar. Browsers list the custom type in `types` during
 * dragover and drop, so this does not depend on the sidebar's own state.
 */
export function isInternalDrag(transfer: DataTransfer | null | undefined): boolean {
  if (!transfer) return false;
  return Array.from(transfer.types).includes(NODE_MIME);
}
