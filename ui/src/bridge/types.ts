import type { ApiResult, Body, CoreEvent } from '../wire';

/**
 * Transport to the core. `HttpBridge` talks to the host; `MockBridge` is an in-memory core for
 * development, tests and screenshots. Both implement the same semantics. Sync, which covers
 * coalescing and stale-response dropping, lives above this interface in `Sidebar`.
 */
export interface Bridge {
  /** Sends one request. Rejects only when the transport itself fails. */
  request(body: Body): Promise<ApiResult>;
  /** Registers a handler for pushed events. Returns the unsubscribe function. */
  onEvent(handler: (event: CoreEvent) => void): () => void;
}

export type EventHandler = (event: CoreEvent) => void;

const TOPICS: readonly string[] = ['tree', 'session', 'settings'];

/** Runtime check for events pushed by the host. The host is trusted, but a bad payload must not break the page. */
export function isCoreEvent(value: unknown): value is CoreEvent {
  if (typeof value !== 'object' || value === null) return false;
  const e = value as { topic?: unknown; rev?: unknown };
  return typeof e.topic === 'string' && TOPICS.includes(e.topic) && typeof e.rev === 'number';
}

declare global {
  interface Window {
    /** Called by the host with `execute_java_script('window.yaacEvent({...})')`. */
    yaacEvent?: (event: CoreEvent) => void;
  }
}
