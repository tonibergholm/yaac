import { API_VERSION, RPC_ENDPOINT, type ApiRequest, type ApiResult, type Body, type CoreEvent } from '../wire';
import {
  BridgeError,
  ProtocolError,
  RPC_TIMEOUT_MS,
  RpcStatusError,
  RpcTimeoutError,
  isCoreEvent,
  type Bridge,
  type EventHandler,
} from './types';
import { parseApiResponse } from './validate';

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
    let response: Response;
    try {
      response = await fetch(RPC_ENDPOINT, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(request),
        signal: AbortSignal.timeout(RPC_TIMEOUT_MS),
      });
    } catch (error) {
      throw transportError(error);
    }
    if (!response.ok) throw new RpcStatusError(response.status);
    let parsed: unknown;
    try {
      parsed = await response.json();
    } catch {
      throw new ProtocolError('rpc reply is not JSON');
    }
    return parseApiResponse(parsed, id);
  }

  onEvent(handler: EventHandler): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }
}

function transportError(error: unknown): BridgeError {
  const name = typeof error === 'object' && error !== null ? (error as { name?: unknown }).name : undefined;
  if (name === 'TimeoutError' || name === 'AbortError') return new RpcTimeoutError('rpc timed out');
  return new BridgeError('rpc request failed');
}
