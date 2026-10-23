import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BridgeError, ProtocolError, RpcStatusError, RpcTimeoutError } from '../src/bridge/types';
import { Sidebar, refusalOfCode, refusalOfError, refusalText, type Refusal } from '../src/state/sidebar';
import type { ApiResult, Body } from '../src/wire';
import { FakeBridge, clone, snapshot, tab } from './support';

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await vi.advanceTimersByTimeAsync(0);
}

function snapshotResult(rev: number): ApiResult {
  return { status: 'ok', body: { type: 'snapshot', ...clone(snapshot({ rev, pinned: [tab('a', 'A')] })) } };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/** Starts a sidebar on rev 5, with the requests made so far cleared. */
async function started(): Promise<{ bridge: FakeBridge; sidebar: Sidebar }> {
  const bridge = new FakeBridge(snapshot({ rev: 5 }));
  const sidebar = new Sidebar(bridge);
  sidebar.start();
  await settle();
  bridge.requests.length = 0;
  return { bridge, sidebar };
}

describe('stop() ends all pending work', () => {
  it('makes no request after stop, even when events, retries or commands follow', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = () => {
      throw new Error('transport down');
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(bridge.queries()).toHaveLength(1); // the failed pull, with a retry scheduled

    sidebar.stop();
    await vi.advanceTimersByTimeAsync(60_000);
    bridge.emit({ topic: 'tree', rev: 9 });
    await settle();
    await sidebar.closeTab('a');
    await sidebar.newTab();
    await vi.advanceTimersByTimeAsync(60_000);

    expect(bridge.requests).toHaveLength(1);
  });

  it('drops a reply that arrives after stop, and does not apply it', async () => {
    const { bridge, sidebar } = await started();
    const pending = deferred<ApiResult>();
    bridge.respond = (body: Body) => (body.kind === 'query' ? pending.promise : { status: 'ok', body: { type: 'ack', rev: 0 } });
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    sidebar.stop();

    pending.resolve(snapshotResult(9));
    await settle();
    expect(sidebar.store.get().snapshot?.rev).toBe(5);
  });

  it('start() after stop() resumes sync', async () => {
    const { bridge, sidebar } = await started();
    sidebar.stop();
    sidebar.start();
    await settle();
    expect(bridge.queries()).toHaveLength(1);
    sidebar.stop();
  });
});

describe('transient and permanent failures', () => {
  it.each([
    ['internal', null],
    ['not_found', null],
    ['conflict', null],
  ])('error code %s is transient: the sidebar reconnects and retries', async (code) => {
    const { bridge, sidebar } = await started();
    bridge.respond = () => ({ status: 'err', error: { code: code as 'internal', message: 'x' } });
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(sidebar.store.get().link).toBe('reconnecting');
    await vi.advanceTimersByTimeAsync(250);
    expect(bridge.queries()).toHaveLength(2);
    sidebar.stop();
  });

  it.each([
    ['unsupported_version', 'version'],
    ['forbidden', 'forbidden'],
    ['bad_request', 'request'],
  ] as const)('error code %s is a permanent refusal: %s, and nothing is retried', async (code, refusal) => {
    const { bridge, sidebar } = await started();
    bridge.respond = () => ({ status: 'err', error: { code, message: 'nope' } });
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(sidebar.store.get().link).toBe('refused');
    expect(sidebar.store.get().refusal).toBe(refusal);

    await vi.advanceTimersByTimeAsync(60_000);
    expect(bridge.queries()).toHaveLength(1);
    sidebar.stop();
  });

  it('events do not revive a refused session, but a manual retry does', async () => {
    const { bridge, sidebar } = await started();
    let refuse = true;
    bridge.respond = (body: Body) => {
      if (body.kind !== 'query') return { status: 'ok', body: { type: 'ack', rev: 0 } };
      return refuse ? { status: 'err', error: { code: 'unsupported_version', message: 'old UI' } } : snapshotResult(9);
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(sidebar.store.get().refusal).toBe('version');

    bridge.emit({ topic: 'tree', rev: 9 });
    await settle();
    expect(bridge.queries()).toHaveLength(1);

    refuse = false;
    sidebar.retry();
    await settle();
    expect(bridge.queries()).toHaveLength(2);
    expect(sidebar.store.get().link).toBe('ok');
    expect(sidebar.store.get().refusal).toBeNull();
    expect(sidebar.store.get().snapshot?.rev).toBe(9);
    sidebar.stop();
  });

  it('a reply the validator rejects is a permanent refusal: reply', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = () => {
      throw new ProtocolError('rpc reply is malformed');
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(sidebar.store.get().link).toBe('refused');
    expect(sidebar.store.get().refusal).toBe('reply');
    sidebar.stop();
  });

  it('a query answered with an ack is a permanent refusal: reply', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = () => ({ status: 'ok', body: { type: 'ack', rev: 6 } });
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(sidebar.store.get().refusal).toBe('reply');
    sidebar.stop();
  });
});

describe('classification of errors', () => {
  it('HTTP 5xx and transport failures are transient; 403 is forbidden; other 4xx is a request refusal', () => {
    expect(refusalOfError(new RpcStatusError(500))).toBeNull();
    expect(refusalOfError(new RpcStatusError(503))).toBeNull();
    expect(refusalOfError(new RpcStatusError(403))).toBe('forbidden');
    expect(refusalOfError(new RpcStatusError(404))).toBe('request');
    expect(refusalOfError(new BridgeError('network'))).toBeNull();
    expect(refusalOfError(new RpcTimeoutError('slow'))).toBeNull();
    expect(refusalOfError(new ProtocolError('bad'))).toBe('reply');
    expect(refusalOfError(new Error('unknown network failure'))).toBeNull();
  });

  it('maps permanent error codes and leaves the rest transient', () => {
    expect(refusalOfCode('unsupported_version')).toBe('version');
    expect(refusalOfCode('forbidden')).toBe('forbidden');
    expect(refusalOfCode('bad_request')).toBe('request');
    expect(refusalOfCode('internal')).toBeNull();
    expect(refusalOfCode('not_found')).toBeNull();
    expect(refusalOfCode('conflict')).toBeNull();
  });

  it('gives each refusal its own message', () => {
    const refusals: Refusal[] = ['version', 'forbidden', 'request', 'reply'];
    const texts = refusals.map(refusalText);
    expect(new Set(texts).size).toBe(refusals.length);
    expect(refusalText('version')).toMatch(/refused this UI version/);
  });
});

describe('command errors', () => {
  it('a command rejected for its own reason shows a notice and does not block the tree', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = (body: Body) =>
      body.kind === 'command'
        ? { status: 'err', error: { code: 'bad_request', message: 'title is too long' } }
        : snapshotResult(5);
    await sidebar.renameNode('a', 'x');
    await settle();
    expect(sidebar.store.get().notice).toBe('title is too long');
    expect(sidebar.store.get().link).toBe('ok');
    sidebar.stop();
  });

  it('a command that the core refuses for the session blocks the page', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = (body: Body) =>
      body.kind === 'command'
        ? { status: 'err', error: { code: 'unsupported_version', message: 'old UI' } }
        : snapshotResult(5);
    await sidebar.newTab();
    expect(sidebar.store.get().refusal).toBe('version');
    sidebar.stop();
  });

  it('a notice survives a later successful pull and is cleared by the next successful command', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = (body: Body) =>
      body.kind === 'command'
        ? { status: 'err', error: { code: 'not_found', message: 'that tab is gone' } }
        : snapshotResult(5);
    await sidebar.closeTab('missing');
    await settle();
    // The resync pull succeeded. The message the user was shown must stay.
    expect(sidebar.store.get().notice).toBe('that tab is gone');

    bridge.respond = () => ({ status: 'ok', body: { type: 'ack', rev: 5 } });
    await sidebar.selectTab('a');
    expect(sidebar.store.get().notice).toBeNull();
    sidebar.stop();
  });

  it('a timed-out command with a later failed pull keeps the link in reconnecting', async () => {
    const { bridge, sidebar } = await started();
    bridge.respond = (body: Body) => {
      if (body.kind === 'command') return new Promise<ApiResult>(() => {});
      throw new Error('still down');
    };
    const pending = sidebar.selectTab('a');
    await vi.advanceTimersByTimeAsync(10_000);
    await pending;
    await settle();
    expect(sidebar.store.get().notice).toMatch(/did not answer in time/);
    expect(sidebar.store.get().link).toBe('reconnecting');
    sidebar.stop();
  });
});
