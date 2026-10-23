import { cleanup, createEvent, fireEvent, render, screen, waitFor, within } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../src/ui/App';
import { Sidebar } from '../src/state/sidebar';
import type { Command } from '../src/wire';
import { FakeBridge, folder, snapshot, tab } from './support';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

async function mount(bridge: FakeBridge) {
  const sidebar = new Sidebar(bridge);
  const view = render(<App sidebar={sidebar} />);
  sidebar.start();
  await waitFor(() => expect(sidebar.store.get().snapshot).not.toBeNull());
  return { ...view, sidebar, bridge };
}

function standardTree(): FakeBridge {
  return new FakeBridge(
    snapshot({
      rev: 10,
      selected: 'a',
      pinned: [
        tab('a', 'Alpha', { live: true, url: 'https://alpha.example.com/' }),
        folder('f', 'Work', [tab('c', 'Child', { live: false, unread: true })]),
        tab('m', 'Mail'),
      ],
      today: [tab('t', 'Today tab')],
    }),
  );
}

function item(name: RegExp | string) {
  return screen.getByRole('treeitem', { name });
}

function row(label: string): HTMLElement {
  const el = screen.getByText(label, { selector: '.title' }).closest('.row');
  if (!(el instanceof HTMLElement)) throw new Error(`no row for ${label}`);
  return el;
}

function dataTransfer() {
  return { setData: vi.fn(), getData: () => '', effectAllowed: 'all', dropEffect: 'none', types: [] };
}

function stubRect(el: HTMLElement, height = 100): void {
  vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
    top: 0,
    bottom: height,
    left: 0,
    right: 200,
    width: 200,
    height,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}

function dispatchAt(type: 'dragOver' | 'drop', el: Element, clientY: number): boolean {
  const event = type === 'dragOver' ? createEvent.dragOver(el, { dataTransfer: dataTransfer() }) : createEvent.drop(el, { dataTransfer: dataTransfer() });
  Object.defineProperty(event, 'clientY', { value: clientY });
  return fireEvent(el, event);
}

describe('rendering', () => {
  it('renders containers as nested ARIA tree groups with levels and expansion state', async () => {
    await mount(standardTree());
    expect(screen.getByRole('tree', { name: 'Tabs' })).toBeTruthy();
    expect(screen.getAllByRole('group').length).toBeGreaterThanOrEqual(2);

    const work = item(/^Work/);
    expect(work.getAttribute('aria-level')).toBe('1');
    expect(work.getAttribute('aria-expanded')).toBe('true');
    expect(item(/^Child/).getAttribute('aria-level')).toBe('2');

    fireEvent.click(row('Work'));
    expect(item(/^Work/).getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('Child', { selector: '.title' })).toBeNull();
  });

  it('marks the selected tab, distinguishes live from suspended, and flags unread tabs', async () => {
    await mount(standardTree());
    expect(item(/^Alpha/).getAttribute('aria-selected')).toBe('true');
    expect(item(/^Alpha/).classList.contains('suspended')).toBe(false);
    expect(item(/^Mail/).classList.contains('suspended')).toBe(true);
    expect(item(/^Child/).getAttribute('aria-label')).toContain('unread');
    expect(item(/^Child/).getAttribute('aria-label')).toContain('suspended');
  });

  it('shows the hostname when the title is empty and keeps the full address as a tooltip', async () => {
    await mount(
      new FakeBridge(
        snapshot({ rev: 2, today: [tab('e', '', { url: 'https://news.example.org/story?id=1' })] }),
      ),
    );
    expect(screen.getByText('news.example.org', { selector: '.title' })).toBeTruthy();
    expect(screen.getByText('news.example.org', { selector: '.title' }).getAttribute('title')).toBe(
      'https://news.example.org/story?id=1',
    );
  });
});

describe('untrusted page text', () => {
  it('renders a title containing markup as text, never as HTML', async () => {
    const payload = '<img src=x onerror="window.__pwned = true">';
    const { container } = await mount(
      new FakeBridge(snapshot({ rev: 2, today: [tab('x', payload), tab('y', '<script>window.__pwned=1</script>')] })),
    );
    expect(screen.getByText(payload, { selector: '.title' })).toBeTruthy();
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('script')).toBeNull();
    expect((window as unknown as { __pwned?: unknown }).__pwned).toBeUndefined();
  });

  it('shows a javascript: address as text and creates no link', async () => {
    const { container } = await mount(
      new FakeBridge(snapshot({ rev: 2, today: [tab('j', '', { url: 'javascript:alert(1)' })] })),
    );
    expect(screen.getByText('javascript:alert(1)', { selector: '.title' })).toBeTruthy();
    expect(container.querySelector('a')).toBeNull();
  });
});

describe('actions send the exact Command', () => {
  it('new tab, close and select', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.click(screen.getByRole('button', { name: 'New tab' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close Mail' }));
    fireEvent.click(row('Today tab'));
    await waitFor(() => expect(bridge.commands().length).toBe(3));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'open_tab',
        placement: { space: 'space-1', container: 'today', parent: null, before: null },
        url: 'about:blank',
        activate: true,
      },
      { type: 'close_tab', node: 'm' },
      { type: 'select_tab', node: 't' },
    ]);
  });

  it('new folder is created from an inline name', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.click(screen.getByRole('button', { name: 'New folder' }));
    const input = screen.getByRole('textbox', { name: 'Folder name' });
    expect(document.activeElement).toBe(input);
    fireEvent.input(input, { target: { value: 'Reading' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'create_folder',
        placement: { space: 'space-1', container: 'today', parent: null, before: null },
        title: 'Reading',
      },
    ]);
  });

  it('new space is created from an inline name, and the active space is marked', async () => {
    const { bridge } = await mount(standardTree());
    expect(screen.getByText('Personal').getAttribute('aria-current')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'New space' }));
    const input = screen.getByRole('textbox', { name: 'Space name' });
    expect(document.activeElement).toBe(input);
    fireEvent.input(input, { target: { value: 'Work' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'create_space', name: 'Work' }]);
  });

  it('address field navigates the selected tab to an allowed URL', async () => {
    const { bridge } = await mount(standardTree());
    const address = screen.getByRole('textbox', { name: 'Address' }) as HTMLInputElement;
    expect(address.value).toBe('https://alpha.example.com/');
    fireEvent.input(address, { target: { value: 'news.example.org' } });
    fireEvent.keyDown(address, { key: 'Enter' });
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      { type: 'navigate', node: 'a', url: 'https://news.example.org/' },
    ]);
  });

  it('address field rejects javascript: and file: with an inline error and sends nothing', async () => {
    const { bridge } = await mount(standardTree());
    const address = screen.getByRole('textbox', { name: 'Address' });
    for (const value of ['javascript:alert(1)', 'file:///etc/passwd']) {
      fireEvent.input(address, { target: { value } });
      fireEvent.keyDown(address, { key: 'Enter' });
      expect(await screen.findByRole('alert')).toBeTruthy();
      expect(address.getAttribute('aria-invalid')).toBe('true');
    }
    expect(bridge.commands()).toStrictEqual([]);
  });

  it('rename by F2 commits a changed title with Enter and cancels on Escape', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    const input = screen.getByRole('textbox', { name: 'Rename' });
    expect(document.activeElement).toBe(input);
    fireEvent.input(input, { target: { value: 'Post' } });
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(bridge.commands()).toStrictEqual([]);

    fireEvent.keyDown(item(/^Mail/), { key: 'F2' });
    const again = screen.getByRole('textbox', { name: 'Rename' });
    fireEvent.input(again, { target: { value: 'Post' } });
    fireEvent.keyDown(again, { key: 'Enter' });
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([{ type: 'rename_node', node: 'm', title: 'Post' }]);
  });
});

describe('keyboard navigation', () => {
  it('arrow keys move focus over visible rows, Right expands, Left collapses', async () => {
    await mount(standardTree());
    const alpha = item(/^Alpha/);
    alpha.focus();

    fireEvent.keyDown(alpha, { key: 'ArrowDown' });
    await waitFor(() => expect(document.activeElement).toBe(item(/^Work/)));

    fireEvent.keyDown(item(/^Work/), { key: 'ArrowLeft' });
    await waitFor(() => expect(item(/^Work/).getAttribute('aria-expanded')).toBe('false'));
    fireEvent.keyDown(item(/^Work/), { key: 'ArrowRight' });
    await waitFor(() => expect(item(/^Work/).getAttribute('aria-expanded')).toBe('true'));

    fireEvent.keyDown(item(/^Work/), { key: 'ArrowRight' });
    await waitFor(() => expect(document.activeElement).toBe(item(/^Child/)));
    fireEvent.keyDown(item(/^Child/), { key: 'ArrowLeft' });
    await waitFor(() => expect(document.activeElement).toBe(item(/^Work/)));
  });

  it('Enter selects a tab; Delete closes it; F2 starts rename; Alt+Arrow moves', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.keyDown(item(/^Mail/), { key: 'Enter' });
    fireEvent.keyDown(item(/^Mail/), { key: 'Delete' });
    fireEvent.keyDown(item(/^Mail/), { key: 'ArrowUp', altKey: true });
    await waitFor(() => expect(bridge.commands().length).toBe(3));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      { type: 'select_tab', node: 'm' },
      { type: 'close_tab', node: 'm' },
      {
        type: 'move_node',
        node: 'm',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'f' },
      },
    ]);
  });

  it('Enter on a folder toggles it instead of sending a command', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.keyDown(item(/^Work/), { key: 'Enter' });
    await waitFor(() => expect(item(/^Work/).getAttribute('aria-expanded')).toBe('false'));
    expect(bridge.commands()).toStrictEqual([]);
  });

  it('gives exactly one row the tab stop', async () => {
    await mount(standardTree());
    const stops = screen.getAllByRole('treeitem').filter((el) => el.tabIndex === 0);
    expect(stops).toHaveLength(1);
  });
});

describe('drag and drop', () => {
  it('drop on the upper half of a row places the node before that row', async () => {
    const { bridge } = await mount(standardTree());
    const target = row('Mail');
    stubRect(target);
    fireEvent.dragStart(row('Today tab'), { dataTransfer: dataTransfer() });
    const notCancelled = dispatchAt('dragOver', target, 10);
    expect(notCancelled).toBe(false); // preventDefault called: this is a valid drop
    dispatchAt('drop', target, 10);
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 't',
        to: { space: 'space-1', container: 'pinned', parent: null, before: 'm' },
      },
    ]);
  });

  it('drop on the middle of a folder appends into it', async () => {
    const { bridge } = await mount(standardTree());
    const target = row('Work');
    stubRect(target);
    fireEvent.dragStart(row('Mail'), { dataTransfer: dataTransfer() });
    dispatchAt('drop', target, 50);
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'm',
        to: { space: 'space-1', container: 'pinned', parent: 'f', before: null },
      },
    ]);
  });

  it('drop on a container area appends at the end of that container', async () => {
    const { bridge } = await mount(standardTree());
    const section = screen.getByText('Today').closest('section');
    if (!section) throw new Error('no today section');
    fireEvent.dragStart(row('Mail'), { dataTransfer: dataTransfer() });
    dispatchAt('dragOver', section, 0);
    dispatchAt('drop', section, 0);
    await waitFor(() => expect(bridge.commands().length).toBe(1));
    expect(bridge.commands()).toStrictEqual<Command[]>([
      {
        type: 'move_node',
        node: 'm',
        to: { space: 'space-1', container: 'today', parent: null, before: null },
      },
    ]);
  });

  it('a folder cannot be dropped onto itself or into its own child', async () => {
    const { bridge } = await mount(standardTree());
    fireEvent.dragStart(row('Work'), { dataTransfer: dataTransfer() });
    const self = row('Work');
    stubRect(self);
    expect(dispatchAt('dragOver', self, 50)).toBe(true); // not cancelled: not a drop target
    const child = row('Child');
    stubRect(child);
    expect(dispatchAt('dragOver', child, 50)).toBe(true);
    dispatchAt('drop', child, 50);
    expect(bridge.commands()).toStrictEqual([]);
  });
});

describe('tree isolation', () => {
  it('lists only pinned and today, not archive, and scopes items to the tree', async () => {
    await mount(
      new FakeBridge(
        snapshot({ rev: 2, pinned: [tab('p', 'Pinned one')], archive: [tab('z', 'Archived')] }),
      ),
    );
    const tree = screen.getByRole('tree');
    expect(within(tree).queryByText('Archived')).toBeNull();
    expect(within(tree).getByText('Pinned one')).toBeTruthy();
  });
});
