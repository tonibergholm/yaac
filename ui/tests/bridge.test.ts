import { afterEach, describe, expect, it, vi } from 'vitest';
import { BridgeError, HttpBridge } from '../src/bridge/http-bridge';
import { MockBridge } from '../src/bridge/mock-bridge';
import type { ApiResult, Body, CoreEvent, Snapshot } from '../src/wire';
import { flush } from './support';

afterEach(() => {
  vi.unstubAllGlobals();
  delete window.yaacEvent;
});

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('HttpBridge', () => {
  it('posts a versioned Request with an incrementing id to the rpc endpoint', async () => {
    const calls: { url: string; init: RequestInit }[] = [];
    const ok: ApiResult = { status: 'ok', body: { type: 'ack', rev: 3 } };
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init: RequestInit) => {
        calls.push({ url, init });
        const id = (JSON.parse(String(init.body)) as { id: number }).id;
        return jsonResponse({ id, result: ok });
      }),
    );

    const bridge = new HttpBridge(window);
    const body: Body = { kind: 'command', body: { type: 'close_tab', node: 'n1' } };
    await expect(bridge.request(body)).resolves.toStrictEqual(ok);
    await bridge.request({ kind: 'query', body: { type: 'snapshot' } });

    expect(calls.map((c) => c.url)).toStrictEqual(['app://ui/api/v1/rpc', 'app://ui/api/v1/rpc']);
    expect(calls[0]?.init.method).toBe('POST');
    expect(calls[0]?.init.headers).toStrictEqual({ 'content-type': 'application/json' });
    expect(JSON.parse(String(calls[0]?.init.body))).toStrictEqual({
      v: 1,
      id: 1,
      body: { kind: 'command', body: { type: 'close_tab', node: 'n1' } },
    });
    expect(JSON.parse(String(calls[1]?.init.body)).id).toBe(2);
  });

  it('rejects a response whose id does not match the request', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse({ id: 99, result: { status: 'ok', body: { type: 'ack', rev: 1 } } })),
    );
    const bridge = new HttpBridge(window);
    await expect(bridge.request({ kind: 'query', body: { type: 'snapshot' } })).rejects.toBeInstanceOf(BridgeError);
  });

  it('rejects on a non-2xx status', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('forbidden', { status: 403 })));
    const bridge = new HttpBridge(window);
    await expect(bridge.request({ kind: 'query', body: { type: 'snapshot' } })).rejects.toThrow(/403/);
  });

  it('delivers window.yaacEvent to subscribers until they unsubscribe, and ignores malformed events', () => {
    const bridge = new HttpBridge(window);
    const seen: CoreEvent[] = [];
    const unsubscribe = bridge.onEvent((event) => seen.push(event));

    window.yaacEvent?.({ topic: 'tree', rev: 7 });
    window.yaacEvent?.({ topic: 'nonsense', rev: 8 } as unknown as CoreEvent);
    window.yaacEvent?.({ topic: 'session' } as unknown as CoreEvent);
    unsubscribe();
    window.yaacEvent?.({ topic: 'tree', rev: 9 });

    expect(seen).toStrictEqual([{ topic: 'tree', rev: 7 }]);
  });
});

describe('MockBridge', () => {
  async function send(bridge: MockBridge, body: Body): Promise<ApiResult> {
    return bridge.request(body);
  }

  it('answers snapshot and since with the same shape the core sends', async () => {
    const bridge = new MockBridge();
    const result = await send(bridge, { kind: 'query', body: { type: 'snapshot' } });
    expect(result.status).toBe('ok');
    if (result.status !== 'ok' || result.body.type !== 'snapshot') throw new Error('expected snapshot');
    const snap: Snapshot = result.body;
    expect(snap.spaces[0]?.containers.map((c) => c.kind)).toStrictEqual(['favorites', 'pinned', 'today', 'archive']);
    expect(snap.selected).not.toBeNull();
  });

  it('applies open, select, rename, move and close, bumping the revision and emitting events', async () => {
    vi.useFakeTimers();
    try {
      const bridge = new MockBridge();
      const events: CoreEvent[] = [];
      bridge.onEvent((event) => events.push(event));
      const snapshotOf = async (): Promise<Snapshot> => {
        const result = await send(bridge, { kind: 'query', body: { type: 'snapshot' } });
        if (result.status !== 'ok' || result.body.type !== 'snapshot') throw new Error('expected snapshot');
        return result.body;
      };
      const before = await snapshotOf();
      const space = before.active_space;

      const opened = await send(bridge, {
        kind: 'command',
        body: {
          type: 'open_tab',
          placement: { space, container: 'today', parent: null, before: null },
          url: 'about:blank',
          activate: true,
        },
      });
      expect(opened.status).toBe('ok');
      if (opened.status !== 'ok' || opened.body.type !== 'opened') throw new Error('expected opened');
      const newNode = opened.body.node;

      const renamed = await send(bridge, {
        kind: 'command',
        body: { type: 'rename_node', node: newNode, title: 'Fresh' },
      });
      expect(renamed.status).toBe('ok');

      const folder = await send(bridge, {
        kind: 'command',
        body: {
          type: 'create_folder',
          placement: { space, container: 'pinned', parent: null, before: null },
          title: 'Reading',
        },
      });
      expect(folder.status).toBe('ok');
      const after = await snapshotOf();
      expect(after.rev).toBeGreaterThan(before.rev);
      expect(after.selected).toBe(newNode);
      const today = after.spaces[0]?.containers.find((c) => c.kind === 'today')?.nodes ?? [];
      expect(today.at(-1)?.title).toBe('Fresh');

      const closed = await send(bridge, { kind: 'command', body: { type: 'close_tab', node: newNode } });
      expect(closed.status).toBe('ok');
      const final = await snapshotOf();
      expect(final.spaces[0]?.containers.find((c) => c.kind === 'archive')?.nodes.map((n) => n.id)).toContain(
        newNode,
      );

      await vi.runAllTimersAsync();
      expect(events.length).toBeGreaterThanOrEqual(4);
      expect(events.every((e) => e.topic === 'tree')).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('rejects a folder moved into itself and a disallowed scheme with the documented error codes', async () => {
    const bridge = new MockBridge();
    const snapResult = await send(bridge, { kind: 'query', body: { type: 'snapshot' } });
    if (snapResult.status !== 'ok' || snapResult.body.type !== 'snapshot') throw new Error('expected snapshot');
    const snap = snapResult.body;
    const pinned = snap.spaces[0]?.containers.find((c) => c.kind === 'pinned')?.nodes ?? [];
    const folder = pinned.find((n) => n.kind === 'folder');
    if (!folder) throw new Error('seed has no folder');

    const cycle = await send(bridge, {
      kind: 'command',
      body: {
        type: 'move_node',
        node: folder.id,
        to: { space: snap.active_space, container: 'pinned', parent: folder.id, before: null },
      },
    });
    expect(cycle).toMatchObject({ status: 'err', error: { code: 'bad_request' } });

    const scheme = await send(bridge, {
      kind: 'command',
      body: { type: 'navigate', node: folder.id, url: 'javascript:alert(1)' },
    });
    expect(scheme).toMatchObject({ status: 'err' });
  });
});

describe('bridge contract', () => {
  it('uses the same ApiResult envelope for ok and err', async () => {
    const bridge = new MockBridge();
    const result = await bridge.request({
      kind: 'command',
      body: { type: 'select_tab', node: 'missing-node' },
    });
    expect(result).toMatchObject({ status: 'err', error: { code: 'not_found' } });
    await flush();
  });
});
