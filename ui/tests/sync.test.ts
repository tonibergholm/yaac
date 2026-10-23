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

/**
 * Advances fake time, then drains zero-delay timers. A timer created exactly at the end of an
 * advance (for example a retry that schedules a pull) would otherwise stay pending.
 */
async function tick(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  await settle();
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

  it('retries a failed pull with doubling backoff and stops once a snapshot arrives', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    let down = true;
    bridge.respond = (body) => {
      if (body.kind === 'query' && down) throw new Error('transport down');
      if (body.kind === 'query') return snapshotResult(snapshot({ rev: 6 }));
      return { status: 'ok', body: { type: 'ack', rev: 0 } };
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(bridge.queries()).toHaveLength(1);
    expect(sidebar.store.get().link).toBe('reconnecting');

    // Attempts at 250 ms, 750 ms, 1750 ms after the first failure: gaps of 250, 500, 1000.
    await tick(249);
    expect(bridge.queries()).toHaveLength(1);
    await tick(1);
    expect(bridge.queries()).toHaveLength(2);
    await tick(499);
    expect(bridge.queries()).toHaveLength(2);
    await tick(1);
    expect(bridge.queries()).toHaveLength(3);
    await tick(999);
    expect(bridge.queries()).toHaveLength(3);
    await tick(1);
    expect(bridge.queries()).toHaveLength(4);

    down = false;
    await tick(2_000);
    expect(bridge.queries()).toHaveLength(5);
    expect(sidebar.store.get().snapshot?.rev).toBe(6);
    expect(sidebar.store.get().link).toBe('ok');

    // No further retries once the link is back.
    await tick(60_000);
    expect(bridge.queries()).toHaveLength(5);
    sidebar.stop();
  });

  it('caps the backoff at 5 s', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;
    bridge.respond = () => {
      throw new Error('transport down');
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    // Gaps 250, 500, 1000, 2000, 4000 are followed by 5000 for every later attempt.
    await tick(250 + 500 + 1_000 + 2_000 + 4_000);
    expect(bridge.queries()).toHaveLength(6);
    await tick(4_999);
    expect(bridge.queries()).toHaveLength(6);
    await tick(1);
    expect(bridge.queries()).toHaveLength(7);
    await tick(5_000);
    expect(bridge.queries()).toHaveLength(8);
    sidebar.stop();
  });

  it('an event that arrives during backoff pulls at once and cancels the scheduled retry', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;
    let down = true;
    bridge.respond = (body) => {
      if (body.kind === 'query' && down) throw new Error('transport down');
      if (body.kind === 'query') return snapshotResult(snapshot({ rev: 9 }));
      return { status: 'ok', body: { type: 'ack', rev: 0 } };
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await settle();
    expect(bridge.queries()).toHaveLength(1);

    // The retry would fire at 250 ms. An event at 100 ms supersedes it.
    await tick(100);
    down = false;
    bridge.emit({ topic: 'tree', rev: 9 });
    await settle();
    expect(bridge.queries()).toHaveLength(2);
    expect(sidebar.store.get().snapshot?.rev).toBe(9);
    expect(sidebar.store.get().link).toBe('ok');

    // The cancelled retry does not fire later.
    await tick(10_000);
    expect(bridge.queries()).toHaveLength(2);
    sidebar.stop();
  });

  it('a request that never answers times out after 10 s and is retried', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    let hang = true;
    bridge.respond = (body) => {
      if (body.kind === 'query' && hang) return new Promise<ApiResult>(() => {});
      if (body.kind === 'query') return snapshotResult(snapshot({ rev: 6 }));
      return { status: 'ok', body: { type: 'ack', rev: 0 } };
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await tick(9_999);
    expect(sidebar.store.get().link).toBe('ok');
    await tick(1);
    expect(sidebar.store.get().link).toBe('reconnecting');
    expect(bridge.queries()).toHaveLength(1);

    hang = false;
    await tick(250);
    expect(bridge.queries()).toHaveLength(2);
    expect(sidebar.store.get().snapshot?.rev).toBe(6);
    expect(sidebar.store.get().link).toBe('ok');
    sidebar.stop();
  });

  it('a command that times out reports an error and triggers a fresh pull', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;

    bridge.respond = (body) => {
      if (body.kind === 'command') return new Promise<ApiResult>(() => {});
      return snapshotResult(snapshot({ rev: 5 }));
    };
    const pending = sidebar.closeTab('t1');
    await tick(10_000);
    await pending;
    expect(sidebar.store.get().notice).toMatch(/did not answer in time/);
    await settle();
    expect(bridge.queries()).toHaveLength(1);
    expect(sidebar.store.get().link).toBe('ok');
    sidebar.stop();
  });

  it('a backoff retry is cancelled by a manual retry, which resets the backoff', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 5 }));
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    bridge.requests.length = 0;
    let down = true;
    bridge.respond = (body) => {
      if (body.kind === 'query' && down) throw new Error('transport down');
      if (body.kind === 'query') return snapshotResult(snapshot({ rev: 6 }));
      return { status: 'ok', body: { type: 'ack', rev: 0 } };
    };
    bridge.emit({ topic: 'tree', rev: 6 });
    await tick(250 + 500 + 1_000);
    expect(bridge.queries()).toHaveLength(4);

    down = false;
    sidebar.retry();
    await settle();
    expect(bridge.queries()).toHaveLength(5);
    expect(sidebar.store.get().snapshot?.rev).toBe(6);

    // A later failure starts again at 250 ms, not at the old delay.
    down = true;
    bridge.emit({ topic: 'tree', rev: 7 });
    await settle();
    await tick(250);
    expect(bridge.queries()).toHaveLength(7);
    sidebar.stop();
  });

  it('shows a failed load with retry, and recovers on retry', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 2 }));
    bridge.respond = () => ({ status: 'err', error: { code: 'internal', message: 'store unavailable' } });
    const sidebar = new Sidebar(bridge);
    sidebar.start();
    await settle();
    expect(sidebar.store.get().load).toBe('failed');
    // `internal` is transient: the sidebar reconnects instead of showing a notice.
    expect(sidebar.store.get().link).toBe('reconnecting');
    expect(sidebar.store.get().notice).toBeNull();

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
