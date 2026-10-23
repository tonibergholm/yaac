import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it } from 'vitest';
import { Sidebar } from '../src/state/sidebar';
import { App } from '../src/ui/App';
import { NODE_MIME } from '../src/ui/drag';
import { guardForeignDrops } from '../src/ui/guards';
import { FakeBridge, folder, snapshot, tab } from './support';

// Registered once for this file, as main.tsx does at start-up.
guardForeignDrops();

afterEach(() => {
  cleanup();
});

/** A DataTransfer stand-in. `types` is what the browser reports during dragover and drop. */
function transfer(types: string[], dropEffect = 'copy') {
  return {
    types,
    dropEffect,
    effectAllowed: 'all',
    getData: () => '',
    setData: () => {},
  };
}

async function mountTree(): Promise<void> {
  const bridge = new FakeBridge(
    snapshot({
      rev: 10,
      pinned: [folder('f', 'Work', [tab('c', 'Child')]), tab('m', 'Mail')],
      today: [tab('t', 'Today tab')],
    }),
  );
  const sidebar = new Sidebar(bridge);
  render(<App sidebar={sidebar} />);
  sidebar.start();
  await waitFor(() => expect(sidebar.store.get().snapshot).not.toBeNull());
}

function dispatch(kind: 'dragEnter' | 'dragOver' | 'drop', target: Element, dt: ReturnType<typeof transfer>) {
  return fireEvent[kind](target, { dataTransfer: dt });
}

describe('foreign drags (files, links, text from outside the page)', () => {
  it.each([['Files'], ['text/uri-list'], ['text/plain']])(
    'cancels dragover with dropEffect none for a %j drag',
    (type) => {
      const dt = transfer([type]);
      const notCancelled = dispatch('dragOver', document.body, dt);
      expect(notCancelled).toBe(false);
      expect(dt.dropEffect).toBe('none');
    },
  );

  it('cancels dragenter for foreign drags', () => {
    const dt = transfer(['text/uri-list']);
    expect(dispatch('dragEnter', document.body, dt)).toBe(false);
  });

  it('cancels a drop, and the page does not navigate', () => {
    const before = window.location.href;
    const dt = transfer(['text/uri-list']);
    expect(dispatch('drop', document.body, dt)).toBe(false);
    expect(window.location.href).toBe(before);
  });

  it('cancels a foreign drag that lands on a tree row, where the row would otherwise be the target', async () => {
    await mountTree();
    const row = screen.getByRole('treeitem', { name: /^Mail/ });
    const dt = transfer(['Files']);
    expect(dispatch('dragOver', row, dt)).toBe(false);
    expect(dt.dropEffect).toBe('none');
    expect(dispatch('drop', row, dt)).toBe(false);
  });

  it('cancels a middle click, which would otherwise open a link in a new window', () => {
    const event = new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 1 });
    expect(document.body.dispatchEvent(event)).toBe(false);
  });
});

describe('internal tree drags still follow the drop targets', () => {
  it('a valid row accepts the drag, and the window guard does not interfere', async () => {
    await mountTree();
    const dragged = screen.getByRole('treeitem', { name: /^Today tab/ });
    const target = screen.getByRole('treeitem', { name: /^Mail/ });
    const dt = transfer([NODE_MIME], 'none');
    fireEvent.dragStart(dragged.querySelector('.row') as Element, { dataTransfer: dt });
    const row = target.querySelector('.row') as HTMLElement;
    row.getBoundingClientRect = () => ({ top: 0, height: 100, bottom: 100, left: 0, right: 0, width: 0, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
    // Dropping Today tab after Mail is a valid move into Pinned.
    const notCancelled = fireEvent.dragOver(row, { dataTransfer: dt });
    expect(notCancelled).toBe(false);
    expect(dt.dropEffect).toBe('move');
  });

  it('an invalid target leaves dragover uncancelled, so the browser shows the invalid cursor', async () => {
    await mountTree();
    const folderRow = screen.getByRole('treeitem', { name: /^Work/ });
    const childRow = screen.getByRole('treeitem', { name: /^Child/ });
    const dt = transfer([NODE_MIME], 'none');
    fireEvent.dragStart(folderRow.querySelector('.row') as Element, { dataTransfer: dt });
    // Dropping a folder onto its own child is invalid. The row does not cancel the drag, and the
    // window guard leaves internal drags alone, so the drop is refused.
    const notCancelled = fireEvent.dragOver(childRow.querySelector('.row') as Element, { dataTransfer: dt });
    expect(notCancelled).toBe(true);
    expect(dt.dropEffect).toBe('none');
  });
});
