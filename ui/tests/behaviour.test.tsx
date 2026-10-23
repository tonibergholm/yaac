import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it } from 'vitest';
import { App } from '../src/ui/App';
import { Sidebar } from '../src/state/sidebar';
import type { ApiResult, Command, Body } from '../src/wire';
import { FakeBridge, folder, snapshot, tab } from './support';

afterEach(() => {
  cleanup();
});

async function mount(bridge: FakeBridge): Promise<Sidebar> {
  const sidebar = new Sidebar(bridge);
  render(<App sidebar={sidebar} />);
  sidebar.start();
  await waitFor(() => expect(sidebar.store.get().snapshot).not.toBeNull());
  return sidebar;
}

/** pinned: Alpha, Work[Child], Mail. today: Today tab. Selected: Alpha. */
function tree(): FakeBridge {
  return new FakeBridge(
    snapshot({
      rev: 10,
      selected: 'a',
      pinned: [tab('a', 'Alpha'), folder('f', 'Work', [tab('c', 'Child')]), tab('m', 'Mail')],
      today: [tab('t', 'Today tab')],
    }),
  );
}

function item(name: RegExp | string): HTMLElement {
  return screen.getByRole('treeitem', { name });
}

function commandsOf(bridge: FakeBridge): Command[] {
  return bridge.commands();
}

describe('Alt+Shift keyboard moves send the exact placement', () => {
  it('Alt+Shift+Left moves a child out of its folder, after the folder', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Child/), { key: 'ArrowLeft', altKey: true, shiftKey: true });
    await waitFor(() => expect(bridge.commands()).toHaveLength(1));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'c',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'm' },
      },
    ]);
  });

  it('Alt+Shift+Right moves a node into the folder directly above it', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Mail/), { key: 'ArrowRight', altKey: true, shiftKey: true });
    await waitFor(() => expect(bridge.commands()).toHaveLength(1));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'm',
        to: { space: 'space-1', container: 'pinned', parent: 'f', before: null },
      },
    ]);
  });

  it('Alt+Shift+Right does nothing when the node above is not a folder', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Alpha/), { key: 'ArrowRight', altKey: true, shiftKey: true });
    fireEvent.keyDown(item(/^Work/), { key: 'ArrowRight', altKey: true, shiftKey: true });
    await waitFor(() => expect(bridge.commands()).toHaveLength(0));
  });

  it('Alt+Shift+P sends a tab to the end of Pinned, and Alt+Shift+T to the end of Today', async () => {
    const bridge = tree();
    await mount(bridge);
    // `key` is what macOS produces with Alt held. `code` is the physical key.
    fireEvent.keyDown(item(/^Today tab/), { key: 'π', code: 'KeyP', altKey: true, shiftKey: true });
    fireEvent.keyDown(item(/^Mail/), { key: 'Ω', code: 'KeyT', altKey: true, shiftKey: true });
    await waitFor(() => expect(bridge.commands()).toHaveLength(2));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 't',
        to: { space: 'space-1', container: 'pinned', parent: null, before: null },
      },
      {
        type: 'move_node',
        node: 'm',
        to: { space: 'space-1', container: 'today', parent: null, before: null },
      },
    ]);
  });

  it('Alt+Shift+Down reorders past the next sibling', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Alpha/), { key: 'ArrowDown', altKey: true, shiftKey: true });
    await waitFor(() => expect(bridge.commands()).toHaveLength(1));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'a',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'm' },
      },
    ]);
  });

  it('plain Alt+Arrow is not bound and is not treated as a plain arrow', async () => {
    const bridge = tree();
    await mount(bridge);
    const alpha = item(/^Alpha/);
    alpha.focus();
    const notPrevented = fireEvent.keyDown(alpha, { key: 'ArrowDown', altKey: true });
    expect(notPrevented).toBe(true);
    expect(bridge.commands()).toStrictEqual([]);
    expect(document.activeElement).toBe(alpha);
  });
});

describe('focus after close and rename', () => {
  it('clicking close moves focus to the next row, not to the body', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.click(screen.getByRole('button', { name: 'Close Mail' }));
    await waitFor(() => expect(document.activeElement).toBe(item(/^Today tab/)));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([{ type: 'close_tab', node: 'm' }]);
  });

  it('closing the only row moves focus to the New tab button', async () => {
    const bridge = new FakeBridge(snapshot({ rev: 3, today: [tab('only', 'Only tab')] }));
    await mount(bridge);
    fireEvent.click(screen.getByRole('button', { name: 'Close Only tab' }));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'New tab' }));
  });

  it('committing a rename with Enter returns focus to the renamed row', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    const input = screen.getByRole('textbox', { name: 'Rename' });
    fireEvent.input(input, { target: { value: 'Post' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(document.activeElement).toBe(item(/^Mail/)));
  });

  it('cancelling a rename with Escape returns focus to the row', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Rename' }), { key: 'Escape' });
    await waitFor(() => expect(document.activeElement).toBe(item(/^Mail/)));
  });

  it('a blur with nowhere to go commits and returns focus to the row', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    const input = screen.getByRole('textbox', { name: 'Rename' });
    fireEvent.input(input, { target: { value: 'Post' } });
    // A real blur with nowhere to go: jsdom reports relatedTarget null.
    input.blur();
    await waitFor(() => expect(document.activeElement).toBe(item(/^Mail/)));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([{ type: 'rename_node', node: 'm', title: 'Post' }]);
  });

  it('a blur into another control commits but does not take focus back', async () => {
    const bridge = tree();
    await mount(bridge);
    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    const input = screen.getByRole('textbox', { name: 'Rename' });
    const other = screen.getByRole('button', { name: 'New tab' });
    fireEvent.input(input, { target: { value: 'Post' } });
    // Moving focus to another control blurs the input with relatedTarget set.
    other.focus();
    await waitFor(() => expect(commandsOf(bridge)).toHaveLength(1));
    expect(document.activeElement).toBe(other);
  });
});

describe('Space switching', () => {
  function twoSpaces(): FakeBridge {
    return new FakeBridge(
      snapshot({
        rev: 4,
        selected: 'p1',
        pinned: [tab('p1', 'Personal tab')],
        spaces: [
          { id: 's1', name: 'Personal' },
          { id: 's2', name: 'Work' },
        ],
        activeSpace: 's1',
      }),
    );
  }

  it('clicking another Space sends select_space; clicking the active one sends nothing', async () => {
    const bridge = twoSpaces();
    await mount(bridge);
    expect(screen.getByRole('button', { name: 'Personal' }).getAttribute('aria-current')).toBe('true');
    expect(screen.getByRole('button', { name: 'Work' }).getAttribute('aria-current')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Personal' }));
    fireEvent.click(screen.getByRole('button', { name: 'Work' }));
    await waitFor(() => expect(bridge.commands()).toHaveLength(1));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([{ type: 'select_space', space: 's2' }]);
  });

  it('a new Space is created and then switched to', async () => {
    const bridge = twoSpaces();
    const fallback = bridge.respond;
    bridge.respond = (body: Body): ApiResult | Promise<ApiResult> => {
      if (body.kind === 'command' && body.body.type === 'create_space') {
        return { status: 'ok', body: { type: 'space_created', space: 's3', rev: 5 } };
      }
      return fallback(body);
    };
    await mount(bridge);
    fireEvent.click(screen.getByRole('button', { name: 'New space' }));
    const field = screen.getByRole('textbox', { name: 'Space name' });
    expect(document.activeElement).toBe(field);
    fireEvent.input(field, { target: { value: 'Research' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    await waitFor(() => expect(bridge.commands()).toHaveLength(2));
    expect(commandsOf(bridge)).toStrictEqual<Command[]>([
      { type: 'create_space', name: 'Research' },
      { type: 'select_space', space: 's3' },
    ]);
  });
});

describe('reconnecting state', () => {
  it('shows a non-blocking reconnecting line and keeps the last known tree', async () => {
    const bridge = tree();
    const sidebar = await mount(bridge);
    bridge.respond = () => {
      throw new Error('transport down');
    };
    bridge.emit({ topic: 'tree', rev: 11 });
    expect(await screen.findByRole('status')).toBeTruthy();
    expect(screen.getByText(/Reconnecting to the browser core/)).toBeTruthy();
    expect(item(/^Alpha/)).toBeTruthy();
    expect(sidebar.store.get().link).toBe('reconnecting');
    sidebar.stop();
  });

  it('removes the line once a pull succeeds', async () => {
    const bridge = tree();
    const sidebar = await mount(bridge);
    let down = true;
    bridge.respond = (body) => {
      if (body.kind === 'query' && down) throw new Error('transport down');
      if (body.kind === 'query') return { status: 'ok', body: { type: 'snapshot', ...bridge.snap } };
      return { status: 'ok', body: { type: 'ack', rev: 11 } };
    };
    bridge.emit({ topic: 'tree', rev: 11 });
    await screen.findByRole('status');
    down = false;
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull(), { timeout: 3000 });
    sidebar.stop();
  });
});
