import { useSyncExternalStore } from 'preact/compat';
import { activeSpace, locate } from '../model/tree';
import type { Sidebar } from '../state/sidebar';
import { AddressBar } from './AddressBar';
import { SpaceSwitcher } from './SpaceSwitcher';
import { Toolbar } from './Toolbar';
import { Tree } from './Tree';

export function App({ sidebar }: { sidebar: Sidebar }) {
  const state = useSyncExternalStore(sidebar.store.subscribe, sidebar.store.get);
  const notice =
    state.notice === null ? null : (
      <p class="notice" role="alert">
        {state.notice}{' '}
        <button type="button" class="link-button" onClick={() => sidebar.dismissNotice()}>
          Dismiss
        </button>
      </p>
    );

  const snapshot = state.snapshot;
  if (snapshot === null) {
    return (
      <div class="sidebar">
        {state.load === 'failed' ? (
          <div class="status">
            <p>Tabs could not be loaded.</p>
            <button type="button" class="button" onClick={() => sidebar.retry()}>
              Retry
            </button>
          </div>
        ) : (
          <p class="status">Loading tabs</p>
        )}
        {notice}
      </div>
    );
  }

  const space = activeSpace(snapshot);
  const selected =
    space && snapshot.selected !== null ? (locate(space, snapshot.selected)?.node ?? null) : null;

  return (
    <div class="sidebar">
      <SpaceSwitcher sidebar={sidebar} snapshot={snapshot} />
      <AddressBar key={selected?.id ?? 'none'} sidebar={sidebar} selected={selected} />
      <Toolbar sidebar={sidebar} />
      {space ? (
        <Tree sidebar={sidebar} state={state} snapshot={snapshot} space={space} />
      ) : (
        <p class="status">No Space is active.</p>
      )}
      {notice}
    </div>
  );
}
