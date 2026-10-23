import { afterEach, describe, expect, it, vi } from 'vitest';
import { HttpBridge } from '../src/bridge/http-bridge';
import { BridgeError, RpcTimeoutError, isCoreEvent } from '../src/bridge/types';
import type { Body } from '../src/wire';

afterEach(() => {
  vi.unstubAllGlobals();
  delete window.yaacEvent;
});

const query: Body = { kind: 'query', body: { type: 'snapshot' } };

function json(payload: unknown): Response {
  return new Response(JSON.stringify(payload), { status: 200, headers: { 'content-type': 'application/json' } });
}

const goodSnapshot = {
  rev: 3,
  window: 'w1',
  active_space: 's1',
  selected: null,
  spaces: [{ id: 's1', name: 'Personal', theme: null, containers: [{ kind: 'pinned', nodes: [] }] }],
};

describe('HttpBridge timeouts and errors', () => {
  it('attaches an abort signal to every request', async () => {
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      const id = (JSON.parse(String(init.body)) as { id: number }).id;
      return json({ id, result: { status: 'ok', body: { type: 'snapshot', ...goodSnapshot } } });
    });
    vi.stubGlobal('fetch', fetchMock);
    await new HttpBridge(window).request(query);
    const init = fetchMock.mock.calls[0]?.[1];
    expect(init?.signal).toBeDefined();
  });

  it('maps an abort or timeout from fetch to RpcTimeoutError', async () => {
    const timeout = Object.assign(new Error('timed out'), { name: 'TimeoutError' });
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(timeout)));
    await expect(new HttpBridge(window).request(query)).rejects.toBeInstanceOf(RpcTimeoutError);
  });

  it('maps any other fetch failure to BridgeError', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('network down'))));
    const error = await new HttpBridge(window).request(query).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(BridgeError);
    expect(error).not.toBeInstanceOf(RpcTimeoutError);
  });

  it('turns a body that is not JSON into a BridgeError', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>', { status: 200 })));
    await expect(new HttpBridge(window).request(query)).rejects.toBeInstanceOf(BridgeError);
  });
});

describe('HttpBridge validates replies', () => {
  function replyWith(result: unknown) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init: RequestInit) => {
        const id = (JSON.parse(String(init.body)) as { id: number }).id;
        return json({ id, result });
      }),
    );
    return new HttpBridge(window).request(query);
  }

  it('accepts a well-formed snapshot, ack, opened and error', async () => {
    await expect(replyWith({ status: 'ok', body: { type: 'snapshot', ...goodSnapshot } })).resolves.toBeDefined();
    await expect(replyWith({ status: 'ok', body: { type: 'ack', rev: 4 } })).resolves.toBeDefined();
    await expect(replyWith({ status: 'ok', body: { type: 'opened', node: 'n1', rev: 5 } })).resolves.toBeDefined();
    await expect(
      replyWith({ status: 'err', error: { code: 'not_found', message: 'gone' } }),
    ).resolves.toBeDefined();
  });

  it.each([
    ['unknown body type', { status: 'ok', body: { type: 'mystery', rev: 1 } }],
    ['rev that is not an integer', { status: 'ok', body: { type: 'ack', rev: 1.5 } }],
    ['negative rev', { status: 'ok', body: { type: 'ack', rev: -1 } }],
    ['snapshot without spaces', { status: 'ok', body: { type: 'snapshot', rev: 1, window: 'w', active_space: 's', selected: null } }],
    [
      'node with an unknown kind',
      {
        status: 'ok',
        body: {
          type: 'snapshot',
          ...goodSnapshot,
          spaces: [
            {
              id: 's1',
              name: 'P',
              theme: null,
              containers: [
                {
                  kind: 'pinned',
                  nodes: [{ id: 'x', kind: 'widget', title: 't', url: null, is_unread: false, live: false, children: [] }],
                },
              ],
            },
          ],
        },
      },
    ],
    ['error with an unknown code', { status: 'err', error: { code: 'teapot', message: 'no' } }],
    ['neither ok nor err', { status: 'maybe' }],
    ['missing result', undefined],
  ])('rejects a reply with %s as a BridgeError', async (_name, result) => {
    const error = await replyWith(result).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(BridgeError);
  });

  it('rejects a reply that answers a different request id', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => json({ id: 999, result: { status: 'ok', body: { type: 'ack', rev: 1 } } })),
    );
    await expect(new HttpBridge(window).request(query)).rejects.toBeInstanceOf(BridgeError);
  });
});

describe('isCoreEvent', () => {
  it('requires a known topic and a safe non-negative integer revision', () => {
    expect(isCoreEvent({ topic: 'tree', rev: 7 })).toBe(true);
    expect(isCoreEvent({ topic: 'tree', rev: 1.5 })).toBe(false);
    expect(isCoreEvent({ topic: 'tree', rev: Number.MAX_SAFE_INTEGER + 2 })).toBe(false);
    expect(isCoreEvent({ topic: 'tree', rev: -1 })).toBe(false);
    expect(isCoreEvent({ topic: 'tree', rev: '7' })).toBe(false);
    expect(isCoreEvent({ topic: 'other', rev: 7 })).toBe(false);
  });
});
