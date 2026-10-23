import { API_VERSION, RPC_ENDPOINT, type ApiRequest, type ApiResponse, type ApiResult, type Body, type CoreEvent } from '../wire';
import { isCoreEvent, type Bridge, type EventHandler } from './types';

export class BridgeError extends Error {}

/**
 * Talks to the Rust host. Requests are `POST app://ui/api/v1/rpc` with a JSON `Request`. Events
 * arrive through `window.yaacEvent`, which the host calls with `execute_java_script`.
 */
export class HttpBridge implements Bridge {
  private nextId = 1;
  private readonly handlers = new Set<EventHandler>();

  constructor(target: Window = window) {
    target.yaacEvent = (event: CoreEvent) => {
      if (!isCoreEvent(event)) return;
      for (const handler of [...this.handlers]) handler(event);
    };
  }

  async request(body: Body): Promise<ApiResult> {
    const id = this.nextId++;
    const request: ApiRequest = { v: API_VERSION, id, body };
    const response = await fetch(RPC_ENDPOINT, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(request),
    });
    if (!response.ok) throw new BridgeError(`rpc failed with HTTP ${response.status}`);
    const parsed = (await response.json()) as ApiResponse;
    if (parsed.id !== id) throw new BridgeError(`rpc response id ${parsed.id} does not match request ${id}`);
    return parsed.result;
  }

  onEvent(handler: EventHandler): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }
}
