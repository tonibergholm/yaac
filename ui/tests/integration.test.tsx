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

    fireEvent.keyDown(mail, { key: 'ArrowDown', altKey: true });

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
  it('cancels a drop that did not start inside the sidebar, so the page cannot navigate', async () => {
    await mountMock();
    expect(fireEvent.dragOver(document.body)).toBe(false);
    expect(fireEvent.drop(document.body)).toBe(false);
  });
});
