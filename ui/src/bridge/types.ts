import type { ApiResult, Body, CoreEvent } from '../wire';

/** A request that the transport could not complete with a usable reply. Transient: a retry may succeed. */
export class BridgeError extends Error {}

/** The core did not answer within `RPC_TIMEOUT_MS`. The request may still have been applied. Transient. */
export class RpcTimeoutError extends BridgeError {}

/** The host answered with a non-2xx HTTP status. 5xx is transient. 4xx is classified by the sidebar. */
export class RpcStatusError extends BridgeError {
  constructor(readonly status: number) {
    super(`rpc failed with HTTP ${status}`);
  }
}

/**
 * The reply is not one this UI can read: not JSON, the wrong request id, or a shape the validator
 * rejects. Permanent: retrying returns the same reply.
 */
export class ProtocolError extends BridgeError {}

export const RPC_TIMEOUT_MS = 10_000;

/**
 * Transport to the core. `HttpBridge` talks to the host; `MockBridge` is an in-memory core for
 * development, tests and screenshots. Both implement the same semantics. Sync, which covers
 * timeouts, backoff, coalescing and stale-response dropping, lives above this interface in `Sidebar`.
 */
export interface Bridge {
  /** Sends one request. Rejects with a `BridgeError` when no usable reply arrives. */
  request(body: Body): Promise<ApiResult>;
  /** Registers a handler for pushed events. Returns the unsubscribe function. */
  onEvent(handler: (event: CoreEvent) => void): () => void;
}

export type EventHandler = (event: CoreEvent) => void;

const TOPICS: readonly string[] = ['tree', 'session', 'settings'];

/** Runtime check for events pushed by the host. A malformed payload must not break the page. */
export function isCoreEvent(value: unknown): value is CoreEvent {
  if (typeof value !== 'object' || value === null) return false;
  const e = value as { topic?: unknown; rev?: unknown };
  return (
    typeof e.topic === 'string' && TOPICS.includes(e.topic) && Number.isSafeInteger(e.rev) && (e.rev as number) >= 0
  );
}

declare global {
  interface Window {
    /** Called by the host with `execute_java_script('window.yaacEvent({...})')`. */
    yaacEvent?: (event: CoreEvent) => void;
  }
}
