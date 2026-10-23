import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Sidebar } from '../src/state/sidebar';
import type { Command } from '../src/wire';
import { FakeBridge, folder, snapshot, tab } from './support';

// Every expected value is typed as `Command` or `Command[]`, so a field added, renamed or
// removed in yaac-api fails `tsc --noEmit` here, not only at runtime.

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await vi.advanceTimersByTimeAsync(0);
}

let bridge: FakeBridge;
let sidebar: Sidebar;

beforeEach(async () => {
  vi.useFakeTimers();
  bridge = new FakeBridge(
    snapshot({
      rev: 10,
      selected: 'a',
      pinned: [tab('a', 'Alpha'), folder('f', 'Work', [tab('c', 'Child')])],
      today: [tab('t', 'Today')],
      spaces: [{ id: 'space-1', name: 'Personal' }],
    }),
  );
  sidebar = new Sidebar(bridge);
  sidebar.start();
  await settle();
  bridge.requests.length = 0;
});

afterEach(() => {
  sidebar.stop();
  vi.useRealTimers();
});

describe('Sidebar actions send exact Command JSON', () => {
  it('newTab opens about:blank into today of the active Space and activates it', async () => {
    await sidebar.newTab();
    const expected: Command = {
      type: 'open_tab',
      placement: { space: 'space-1', container: 'today', parent: null, before: null },
      url: 'about:blank',
      activate: true,
    };
    expect(bridge.commands()).toStrictEqual([expected]);
  });

  it('selectTab, closeTab', async () => {
    await sidebar.selectTab('t');
    await sidebar.closeTab('t');
    expect(bridge.commands()).toStrictEqual<Command[]>([
      { type: 'select_tab', node: 't' },
      { type: 'close_tab', node: 't' },
    ]);
  });

  it('createFolder trims the title and places it at the container root', async () => {
    await sidebar.createFolder('pinned', '  Reading  ');
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'create_folder',
        placement: { space: 'space-1', container: 'pinned', parent: null, before: null },
        title: 'Reading',
      },
    ]);
  });

  it('renameNode trims the title, and an empty title sends nothing', async () => {
    await sidebar.renameNode('a', ' Renamed ');
    await sidebar.renameNode('a', '   ');
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'rename_node', node: 'a', title: 'Renamed' }]);
  });

  it('moveNode sends the placement unchanged', async () => {
    const to = { space: 'space-1', container: 'today' as const, parent: 'f', before: null };
    await sidebar.moveNode('t', to);
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'move_node', node: 't', to }]);
  });

  it('createSpace trims the name', async () => {
    await sidebar.createSpace(' Work ');
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'create_space', name: 'Work' }]);
  });

  it('navigate sends the normalized URL', async () => {
    await sidebar.navigate('a', 'example.com');
    expect(bridge.commands()).toStrictEqual<Command[]>([
      { type: 'navigate', node: 'a', url: 'https://example.com/' },
    ]);
  });

  it('navigate rejects javascript: and file: without sending anything', async () => {
    await sidebar.navigate('a', 'javascript:alert(1)');
    await sidebar.navigate('a', 'file:///etc/passwd');
    expect(bridge.commands()).toStrictEqual([]);
    expect(sidebar.store.get().notice).not.toBeNull();
  });

  it('dropOnRow sends the placement computed from the tree', async () => {
    await sidebar.dropOnRow('t', 'a', 'before');
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 't',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'a' },
      },
    ]);
  });

  it('moveStep sends a sibling move; a move that is not possible sends nothing', async () => {
    await sidebar.moveStep('f', -1);
    await sidebar.moveStep('a', -1);
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'f',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'a' },
      },
    ]);
  });

  it('finishRename sends only a changed, non-empty title', async () => {
    sidebar.finishRename('a', 'Alpha');
    sidebar.finishRename('a', null);
    sidebar.finishRename('a', 'Beta');
    await settle();
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'rename_node', node: 'a', title: 'Beta' }]);
  });
});
