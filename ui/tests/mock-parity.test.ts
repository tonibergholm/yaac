import { describe, expect, it } from 'vitest';
import { MockBridge } from '../src/bridge/mock-bridge';
import type { ApiResult, Command, NodeView, Snapshot } from '../src/wire';

// Scenarios taken from the core's tests (crates/yaac-core/src/tests/core.rs). Each one names the
// core test it mirrors, so a change in the core's semantics shows up here.

async function send(bridge: MockBridge, command: Command): Promise<ApiResult> {
  return bridge.request({ kind: 'command', body: command });
}

async function snap(bridge: MockBridge): Promise<Snapshot> {
  const result = await bridge.request({ kind: 'query', body: { type: 'snapshot' } });
  if (result.status !== 'ok' || result.body.type !== 'snapshot') throw new Error('expected a snapshot');
  return result.body;
}

function container(snapshot: Snapshot, kind: 'pinned' | 'today' | 'archive'): NodeView[] {
  const space = snapshot.spaces.find((s) => s.id === snapshot.active_space);
  return space?.containers.find((c) => c.kind === kind)?.nodes ?? [];
}

function findNode(nodes: NodeView[], predicate: (n: NodeView) => boolean): NodeView | undefined {
  for (const node of nodes) {
    if (predicate(node)) return node;
    const inner = findNode(node.children, predicate);
    if (inner) return inner;
  }
  return undefined;
}

function opened(result: ApiResult): string {
  if (result.status !== 'ok' || result.body.type !== 'opened') throw new Error(`expected opened, got ${JSON.stringify(result)}`);
  return result.body.node;
}

function errorOf(result: ApiResult): { code: string; message: string } {
  if (result.status !== 'err') throw new Error('expected an error');
  return result.error;
}

describe('mock core parity', () => {
  it('moving a folder into its own subtree is a conflict, as in moving_a_folder_into_its_own_subtree_is_a_conflict', async () => {
    const bridge = new MockBridge();
    const space = (await snap(bridge)).active_space;
    const work = findNode(container(await snap(bridge), 'pinned'), (n) => n.title === 'Work');
    if (!work) throw new Error('seed has no Work folder');
    const inner = opened(
      await send(bridge, {
        type: 'create_folder',
        placement: { space, container: 'pinned', parent: work.id, before: null },
        title: 'Inner',
      }),
    );
    const error = errorOf(
      await send(bridge, {
        type: 'move_node',
        node: work.id,
        to: { space, container: 'pinned', parent: inner, before: null },
      }),
    );
    expect(error.code).toBe('conflict');
  });

  it('closing a folder with children is a conflict, and an empty folder is deleted, as in closing_a_folder_with_children', async () => {
    const bridge = new MockBridge();
    const before = await snap(bridge);
    const space = before.active_space;
    const folder = opened(
      await send(bridge, {
        type: 'create_folder',
        placement: { space, container: 'today', parent: null, before: null },
        title: 'Full',
      }),
    );
    const child = opened(
      await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'today', parent: folder, before: null },
        url: 'https://child.example/',
        activate: false,
      }),
    );
    expect(errorOf(await send(bridge, { type: 'close_tab', node: folder })).code).toBe('conflict');

    await send(bridge, { type: 'close_tab', node: child });
    await send(bridge, { type: 'close_tab', node: folder });
    const after = await snap(bridge);
    expect(findNode(container(after, 'today'), (n) => n.id === folder)).toBeUndefined();
  });

  it('closing the selected tab selects its next sibling, as in closing_the_selected_tab_selects_the_previous_sibling', async () => {
    const bridge = new MockBridge();
    const start = await snap(bridge);
    const space = start.active_space;
    const first = opened(
      await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'today', parent: null, before: null },
        url: 'https://one.example/',
        activate: true,
      }),
    );
    const second = opened(
      await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'today', parent: null, before: null },
        url: 'https://two.example/',
        activate: true,
      }),
    );
    await send(bridge, { type: 'select_tab', node: first });
    await send(bridge, { type: 'close_tab', node: first });
    expect((await snap(bridge)).selected).toBe(second);
  });

  it('closing the last tab of a Space opens a new blank tab, as in closing_the_last_tab_opens_a_new_blank_tab', async () => {
    const bridge = new MockBridge();
    const created = await send(bridge, { type: 'create_space', name: 'Solo' });
    if (created.status !== 'ok' || created.body.type !== 'space_created') throw new Error('expected space_created');
    await send(bridge, { type: 'select_space', space: created.body.space });
    const only = (await snap(bridge)).selected;
    if (only === null) throw new Error('a blank tab is selected');
    await send(bridge, { type: 'close_tab', node: only });
    const after = await snap(bridge);
    expect(after.selected).not.toBeNull();
    expect(after.selected).not.toBe(only);
    expect(container(after, 'today').some((n) => n.id === after.selected)).toBe(true);
  });

  it('falls back to a tab inside a folder when no sibling tab is left, as in closing_a_selected_tab_falls_back', async () => {
    const bridge = new MockBridge();
    const created = await send(bridge, { type: 'create_space', name: 'Nested' });
    if (created.status !== 'ok' || created.body.type !== 'space_created') throw new Error('expected space_created');
    await send(bridge, { type: 'select_space', space: created.body.space });
    const space = created.body.space;
    const blank = (await snap(bridge)).selected;
    if (blank === null) throw new Error('a blank tab is selected');

    const folder = opened(
      await send(bridge, {
        type: 'create_folder',
        placement: { space, container: 'today', parent: null, before: null },
        title: 'Group',
      }),
    );
    const inside = opened(
      await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'today', parent: folder, before: null },
        url: 'https://inside.example/',
        activate: false,
      }),
    );
    const last = opened(
      await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'today', parent: null, before: null },
        url: 'https://last.example/',
        activate: true,
      }),
    );
    // Close the blank tab first, so the folder's tab is the first tab of the Space.
    await send(bridge, { type: 'close_tab', node: blank });
    await send(bridge, { type: 'close_tab', node: last });
    expect((await snap(bridge)).selected).toBe(inside);
  });

  it('a split holds two to four tabs and nothing else, as the core enforces', async () => {
    const bridge = new MockBridge({ split: true });
    const before = await snap(bridge);
    const space = before.active_space;
    const split = findNode(container(before, 'pinned'), (n) => n.kind === 'split');
    if (!split) throw new Error('the split seed is missing');

    expect(
      errorOf(
        await send(bridge, {
          type: 'create_folder',
          placement: { space, container: 'pinned', parent: split.id, before: null },
          title: 'Not allowed',
        }),
      ).code,
    ).toBe('bad_request');

    // Two children, then two more: four is the limit.
    for (let i = 0; i < 2; i++) {
      const ok = await send(bridge, {
        type: 'open_tab',
        placement: { space, container: 'pinned', parent: split.id, before: null },
        url: `https://added-${i}.example/`,
        activate: false,
      });
      expect(ok.status).toBe('ok');
    }
    const fifth = await send(bridge, {
      type: 'open_tab',
      placement: { space, container: 'pinned', parent: split.id, before: null },
      url: 'https://too-many.example/',
      activate: false,
    });
    expect(errorOf(fifth).code).toBe('conflict');
  });

  it('a folder cannot be moved into a split', async () => {
    const bridge = new MockBridge({ split: true });
    const before = await snap(bridge);
    const space = before.active_space;
    const pinned = container(before, 'pinned');
    const split = findNode(pinned, (n) => n.kind === 'split');
    const work = findNode(pinned, (n) => n.title === 'Work');
    if (!split || !work) throw new Error('seed is missing');
    expect(
      errorOf(
        await send(bridge, {
          type: 'move_node',
          node: work.id,
          to: { space, container: 'pinned', parent: split.id, before: null },
        }),
      ).code,
    ).toBe('bad_request');
  });
});
