import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it } from 'vitest';
import { MockBridge } from '../src/bridge/mock-bridge';
import { Sidebar } from '../src/state/sidebar';
import { App } from '../src/ui/App';
import { guardForeignDrops } from '../src/ui/guards';

// The full loop, with no fake snapshot: action -> command -> event -> since -> DOM.
guardForeignDrops();

afterEach(() => {
  cleanup();
});

async function mountMock(): Promise<Sidebar> {
  const sidebar = new Sidebar(new MockBridge());
  render(<App sidebar={sidebar} />);
  sidebar.start();
  await waitFor(() => expect(sidebar.store.get().snapshot).not.toBeNull());
  return sidebar;
}

function rows(): HTMLElement[] {
  return screen.getAllByRole('treeitem');
}

describe('core round trip with MockBridge', () => {
  it('a new tab appears after the core acknowledges it, and Delete removes it again', async () => {
    await mountMock();
    const before = rows().length;

    fireEvent.click(screen.getByRole('button', { name: 'New tab' }));
    await waitFor(() => expect(rows().length).toBe(before + 1));
    const created = rows().find((el) => el.getAttribute('aria-selected') === 'true');
    if (!created) throw new Error('new tab is not selected');

    fireEvent.keyDown(created, { key: 'Delete' });
    await waitFor(() => expect(rows().length).toBe(before));
  });

  it('after Alt+Arrow moves a node, the DOM reorders and focus stays on the moved node', async () => {
    await mountMock();
    const mail = screen.getByRole('treeitem', { name: /^Mail/ });
    const mailId = mail.dataset.nodeId;
    mail.focus();

    fireEvent.keyDown(mail, { key: 'ArrowDown', altKey: true, shiftKey: true });

    await waitFor(() => {
      const work = screen.getByRole('treeitem', { name: /^Work/ });
      const moved = screen.getByRole('treeitem', { name: /^Mail/ });
      // DOCUMENT_POSITION_FOLLOWING: Mail now comes after Work.
      expect(work.compareDocumentPosition(moved) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });
    await waitFor(() => expect(document.activeElement?.getAttribute('data-node-id')).toBe(mailId));
  });
});

describe('drops from outside the page', () => {
  it('refuses a foreign drag at dragover and drop, so the page cannot navigate', async () => {
    await mountMock();
    const before = window.location.href;
    // A drag without the sidebar's MIME type is foreign. It is cancelled, so no drop is accepted.
    expect(fireEvent.dragOver(document.body)).toBe(false);
    expect(fireEvent.drop(document.body)).toBe(false);
    expect(window.location.href).toBe(before);
  });
});

describe('folders and Spaces with MockBridge', () => {
  it('new folder is created, then named inline, and the name lands in the tree', async () => {
    await mountMock();
    fireEvent.click(screen.getByRole('button', { name: 'New folder' }));
    const field = screen.getByRole('textbox', { name: 'Folder name' });
    fireEvent.input(field, { target: { value: 'Reading' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    const rename = await screen.findByRole('textbox', { name: 'Rename' });
    await waitFor(() => expect(document.activeElement).toBe(rename));
    fireEvent.input(rename, { target: { value: 'Reading list' } });
    fireEvent.keyDown(rename, { key: 'Enter' });

    await waitFor(() => expect(screen.getByRole('treeitem', { name: /^Reading list/ })).toBeTruthy());
  });

  it('creating a Space switches to it, and the core opens a tab in an empty Space', async () => {
    await mountMock();
    fireEvent.click(screen.getByRole('button', { name: 'New space' }));
    const field = screen.getByRole('textbox', { name: 'Space name' });
    fireEvent.input(field, { target: { value: 'Research' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    await waitFor(() => expect(screen.getByRole('button', { name: 'Research' }).getAttribute('aria-current')).toBe('true'));
    // The new Space has no tabs, so the core opened one.
    await waitFor(() => expect(screen.getByRole('treeitem', { name: /^about:blank/ })).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Personal' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Personal' }).getAttribute('aria-current')).toBe('true'));
    expect(screen.getByRole('treeitem', { name: /^Example Domain/ }).getAttribute('aria-selected')).toBe('true');
  });
});
