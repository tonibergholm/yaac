import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Sidebar } from '../src/state/sidebar';
import type { ApiResult, Body, Snapshot } from '../src/wire';
import { FakeBridge, clone, snapshot, tab } from './support';

// Sync runs on timers, for coalescing, and on promises, for requests. Use fake timers and drain both.
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await vi.advanceTimersByTimeAsync(0);
  }
}

function snapshotResult(snap: Snapshot): ApiResult {
  return { status: 'ok', body: { type: 'snapshot', ...clone(snap) } };
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

describe('Sidebar sync', () => {
  it('loads the initial snapshot with a snapshot query', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5, pinned: [tab('a', 'A')] }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();

    expect(bridge.queries()).toStrictEqual([{ kind: 'query', body: { type: 'snapshot' } }]);
    expect(sidebar.store.get().snapshot?.rev).toBe(5);
    expect(sidebar.store.get().load).toBe('ready');
    sidebar.stop();
  });

  it('coalesces a burst of events into one since pull', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    bridge.snap = snapshot({ rev: 9 });
    for (const rev of [6, 7, 8, 9]) bridge.emit({ topic: 'tree', rev });
    await settle();

    expect(bridge.queries()).toStrictEqual([{ kind: 'query', body: { type: 'since', rev: 5 } }]);
    expect(sidebar.store.get().snapshot?.rev).toBe(9);
    sidebar.stop();
  });

  it('ignores events that are not newer than the applied revision', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    bridge.emit({ topic: 'tree', rev: 4 });
    bridge.emit({ topic: 'tree', rev: 5 });
    await settle();

    expect(bridge.requests).toStrictEqual([]);
    sidebar.stop();
  });

  it('merges events that arrive during an in-flight pull into one follow-up pull', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    const first = deferred<ApiResult>();
    const second = deferred<ApiResult>();
    const queue = [first, second];
    bridge.respond = (body: Body) => {
      if (body.kind === 'query') return queue.shift()!.promise;
      return { status: 'ok', body: { type: 'ack', rev: 0 } };
    };

    bridge.emit({ topic: 'tree', rev: 6 });
    await settle(); // since(5) in flight
    bridge.emit({ topic: 'tree', rev: 7 });
    bridge.emit({ topic: 'tree', rev: 8 });
    await settle(); // both events queue behind the in-flight pull

    first.resolve(snapshotResult(snapshot({ rev: 6 })));
    await settle();
    expect(sidebar.store.get().snapshot?.rev).toBe(6);

    second.resolve(snapshotResult(snapshot({ rev: 8 })));
    await settle();
    expect(sidebar.store.get().snapshot?.rev).toBe(8);
    expect(bridge.queries()).toStrictEqual([
      { kind: 'query', body: { type: 'since', rev: 5 } },
      { kind: 'query', body: { type: 'since', rev: 6 } },
    ]);
    sidebar.stop();
  });

  it('applies a newer snapshot and never a lower revision directly', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();

    bridge.respond = (body) =>
      body.kind === 'query' ? snapshotResult(snapshot({ rev: 3 })) : { status: 'ok', body: { type: 'ack', rev: 0 } };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();

    expect(sidebar.store.get().snapshot?.rev).toBe(5);
    sidebar.stop();
  });

  it('does not retry in a loop after a failed pull until the next event', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    bridge.respond = () => {
      throw new Error('transport down');
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(bridge.queries()).toHaveLength(1);
    expect(sidebar.store.get().notice).not.toBeNull();

    await vi.advanceTimersByTimeAsync(10_000);
    expect(bridge.queries()).toHaveLength(1);
    sidebar.stop();
  });

  it('shows a failed load with retry, and recovers on retry', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 2 }));
    bridge.respond = () => ({ status: 'err', error: { code: 'internal', message: 'store unavailable' } });
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    expect(sidebar.store.get().load).toBe('failed');
    expect(sidebar.store.get().notice).toBe('store unavailable');

    bridge.respond = (body) => (body.kind === 'query' ? snapshotResult(bridge.snap) : { status: 'ok', body: { type: 'ack', rev: 0 } });
    sidebar.retry();
    await settle();
    expect(sidebar.store.get().load).toBe('ready');
    expect(sidebar.store.get().snapshot?.rev).toBe(2);
    sidebar.stop();
  });

  it('resyncs after a rejected command, so the UI does not show a stale tree', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    bridge.respond = (body) => {
      if (body.kind === 'command') return { status: 'err', error: { code: 'not_found', message: 'gone' } };
      return snapshotResult(snapshot({ rev: 5 }));
    };
    await sidebar.closeTab('missing');
    await settle();

    expect(sidebar.store.get().notice).toBe('gone');
    expect(bridge.queries()).toHaveLength(1);
    sidebar.stop();
  });
});
