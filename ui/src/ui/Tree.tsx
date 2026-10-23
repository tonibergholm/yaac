import type { TargetedDragEvent, TargetedKeyboardEvent } from 'preact';
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { DropHint, Sidebar, UiState } from '../state/sidebar';
import {
  containerNodes,
  displayTitle,
  placementAtContainerEnd,
  placementForRow,
  placementForSiblingStep,
  placementIntoPreviousFolder,
  placementOutOfFolder,
  SIDEBAR_CONTAINERS,
  visibleRows,
  zoneForRatio,
  type DropZone,
  type SidebarContainer,
  type VisibleRow,
} from '../model/tree';
import type { NodeId, NodeView, Placement, Snapshot, SpaceView } from '../wire';
import { NODE_MIME } from './drag';
import { NEW_TAB_BUTTON_ID } from './ids';
import { ChevronIcon, CloseIcon, FolderIcon, GlobeIcon } from './icons';

const SECTION_LABEL: Record<SidebarContainer, string> = { pinned: 'Pinned', today: 'Today' };
const EMPTY_LABEL: Record<SidebarContainer, string> = {
  pinned: 'Drop tabs here to pin them',
  today: 'No open tabs',
};

interface ItemContext {
  sidebar: Sidebar;
  state: UiState;
  snapshot: Snapshot;
  selectedId: NodeId | null;
  focusId: NodeId | null;
  register: (id: NodeId, el: HTMLElement | null) => void;
  close: (id: NodeId) => void;
}

function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(' ');
}

function zoneFromPointer(event: TargetedDragEvent<HTMLElement>, kind: NodeView['kind']): DropZone {
  const rect = event.currentTarget.getBoundingClientRect();
  const ratio = rect.height > 0 ? (event.clientY - rect.top) / rect.height : 0.5;
  return zoneForRatio(kind, ratio);
}

interface TreeProps {
  sidebar: Sidebar;
  state: UiState;
  snapshot: Snapshot;
  space: SpaceView;
}

/**
 * The pinned and today containers as an ARIA tree with roving focus.
 *
 * Keys. Arrows move focus. Right and Left expand, collapse, or move to a child or parent.
 * Enter or Space selects a tab or toggles a folder. Delete or Backspace closes a tab. F2 renames.
 * Alt+Shift+Up and Alt+Shift+Down reorder among siblings. Alt+Shift+Left moves out of a folder.
 * Alt+Shift+Right moves into the folder just above. Alt+Shift+P moves to the end of Pinned, and
 * Alt+Shift+T to the end of Today.
 */
export function Tree({ sidebar, state, snapshot, space }: TreeProps) {
  const rowEls = useRef(new Map<NodeId, HTMLElement>());
  const rows = visibleRows(space, state.collapsed);
  const focusId = rows.some((r) => r.node.id === state.focusId) ? state.focusId : (rows[0]?.node.id ?? null);
  const selectedId = snapshot.selected;

  // Runs after every render. Moves DOM focus to the requested row once the rendered snapshot has
  // reached the requested revision, so a reorder cannot land after the focus call.
  useLayoutEffect(() => {
    if (!state.focusRequested || state.focusId === null) return;
    if (snapshot.rev < state.focusRev) return;
    // If the requested row is not rendered (for example it sits in a folder that could not expand),
    // focus the first row instead of letting focus fall to <body>.
    const first = rows[0];
    const el = rowEls.current.get(state.focusId) ?? (first ? rowEls.current.get(first.node.id) : undefined);
    el?.focus();
    sidebar.clearFocusRequest();
  });

  const focusAt = (index: number) => {
    const row = rows[index];
    if (row) sidebar.setFocus(row.node.id, true);
  };

  /** Moves a node, then keeps focus on it once the snapshot that contains the move renders. */
  const moveAndKeepFocus = (id: NodeId, placement: Placement) => {
    void sidebar.moveNode(id, placement).then((result) => {
      if (result?.status === 'ok') sidebar.setFocus(id, true, result.body.rev);
    });
  };

  /**
   * Closes a tab and moves focus first, since the focused row is about to leave the DOM. Focus goes
   * to the next row, else the previous one, else the New tab button.
   */
  const closeNode = (id: NodeId) => {
    const index = rows.findIndex((r) => r.node.id === id);
    const row = rows[index];
    if (!row || row.node.kind !== 'tab') return;
    const neighbour = rows[index + 1] ?? rows[index - 1];
    if (neighbour) sidebar.setFocus(neighbour.node.id, true);
    else document.getElementById(NEW_TAB_BUTTON_ID)?.focus();
    void sidebar.closeTab(id);
  };

  const ctx: ItemContext = {
    sidebar,
    state,
    snapshot,
    selectedId,
    focusId,
    register: (id, el) => {
      if (el) rowEls.current.set(id, el);
      else rowEls.current.delete(id);
    },
    close: closeNode,
  };

  const activate = (row: VisibleRow) => {
    if (row.node.kind === 'folder') {
      sidebar.toggleFolder(row.node.id);
      sidebar.setFocus(row.node.id);
    } else {
      void sidebar.selectTab(row.node.id);
    }
  };

  /** Alt+Shift chords: keyboard moves. Each one sends an exact `move_node` placement, or nothing. */
  const moveByKey = (event: TargetedKeyboardEvent<HTMLDivElement>, id: NodeId): boolean => {
    let placement: Placement | null;
    switch (event.key) {
      case 'ArrowUp':
        placement = placementForSiblingStep(snapshot, id, -1);
        break;
      case 'ArrowDown':
        placement = placementForSiblingStep(snapshot, id, 1);
        break;
      case 'ArrowLeft':
        placement = placementOutOfFolder(snapshot, id);
        break;
      case 'ArrowRight':
        placement = placementIntoPreviousFolder(snapshot, id);
        break;
      default:
        // `event.code` is used for letters because Alt changes `event.key` on macOS.
        if (event.code === 'KeyP') placement = placementAtContainerEnd(snapshot, id, 'pinned');
        else if (event.code === 'KeyT') placement = placementAtContainerEnd(snapshot, id, 'today');
        else return false;
    }
    if (placement) moveAndKeepFocus(id, placement);
    return true;
  };

  const onKeyDown = (event: TargetedKeyboardEvent<HTMLDivElement>) => {
    const target = event.target;
    if (!(target instanceof HTMLElement) || target.getAttribute('role') !== 'treeitem') return;
    const id = target.dataset.nodeId;
    const index = rows.findIndex((r) => r.node.id === id);
    const row = rows[index];
    if (!row) return;

    if (event.altKey && event.shiftKey) {
      if (moveByKey(event, row.node.id)) event.preventDefault();
      return;
    }
    // Plain Alt chords are not bound. Ignore them so they do not act as plain arrows.
    if (event.altKey) return;

    const folder = row.node.kind === 'folder';
    const collapsed = state.collapsed.has(row.node.id);
    switch (event.key) {
      case 'ArrowDown':
        focusAt(index + 1);
        break;
      case 'ArrowUp':
        focusAt(index - 1);
        break;
      case 'Home':
        focusAt(0);
        break;
      case 'End':
        focusAt(rows.length - 1);
        break;
      case 'ArrowRight':
        if (folder && collapsed) sidebar.toggleFolder(row.node.id);
        else if (folder && rows[index + 1]?.parent?.id === row.node.id) focusAt(index + 1);
        break;
      case 'ArrowLeft':
        if (folder && !collapsed) {
          sidebar.toggleFolder(row.node.id);
        } else if (row.parent) {
          const parentIndex = rows.findIndex((r) => r.node.id === row.parent?.id);
          focusAt(parentIndex);
        }
        break;
      case 'Enter':
      case ' ':
        activate(row);
        break;
      case 'Delete':
      case 'Backspace':
        closeNode(row.node.id);
        break;
      case 'F2':
        sidebar.startRename(row.node.id);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const containerDragOver = (event: TargetedDragEvent<HTMLElement>, container: SidebarContainer) => {
    const dragId = state.dragId;
    if (dragId === null || placementAtContainerEnd(snapshot, dragId, container) === null) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    sidebar.setDrop({ target: { kind: 'container', container }, zone: 'inside' });
  };

  const containerDrop = (event: TargetedDragEvent<HTMLElement>, container: SidebarContainer) => {
    event.preventDefault();
    const dragId = state.dragId;
    sidebar.setDrag(null);
    if (dragId !== null) void sidebar.dropAtContainerEnd(dragId, container);
  };

  return (
    <div role="tree" aria-label="Tabs" class="tree" onKeyDown={onKeyDown}>
      {SIDEBAR_CONTAINERS.map((container) => {
        const nodes = containerNodes(space, container);
        const labelId = `section-${container}`;
        const hint = state.drop?.target.kind === 'container' && state.drop.target.container === container;
        return (
          <section
            key={container}
            role="group"
            aria-labelledby={labelId}
            class={cx('section', hint && 'drop-end')}
            onDragOver={(e) => containerDragOver(e, container)}
            onDrop={(e) => containerDrop(e, container)}
          >
            <div id={labelId} class="section-title">
              {SECTION_LABEL[container]}
            </div>
            {nodes.length === 0 ? <p class="empty">{EMPTY_LABEL[container]}</p> : null}
            <ul class="list" role="presentation">
              {nodes.map((node, index) => (
                <TreeItem
                  key={node.id}
                  node={node}
                  level={1}
                  index={index}
                  setSize={nodes.length}
                  ctx={ctx}
                />
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

interface TreeItemProps {
  node: NodeView;
  level: number;
  index: number;
  setSize: number;
  ctx: ItemContext;
}

function TreeItem({ node, level, index, setSize, ctx }: TreeItemProps) {
  const { sidebar, state, snapshot } = ctx;
  const isFolder = node.kind === 'folder';
  const isTab = node.kind === 'tab';
  const collapsed = state.collapsed.has(node.id);
  const selected = isTab && node.id === ctx.selectedId;
  const renaming = state.renamingId === node.id;
  const label = displayTitle(node);
  const hint = dropHintFor(state.drop, node.id);

  const status = isTab ? [node.live ? null : 'suspended', node.is_unread ? 'unread' : null] : [];
  const ariaLabel = [label, ...status.filter((s): s is string => s !== null)].join(', ');

  const onRowClick = () => {
    if (renaming) return;
    if (isFolder) {
      sidebar.toggleFolder(node.id);
      sidebar.setFocus(node.id);
    } else {
      sidebar.setFocus(node.id);
      void sidebar.selectTab(node.id);
    }
  };

  const onDragStart = (event: TargetedDragEvent<HTMLDivElement>) => {
    if (renaming) {
      event.preventDefault();
      return;
    }
    event.stopPropagation();
    if (event.dataTransfer) {
      event.dataTransfer.setData(NODE_MIME, node.id);
      event.dataTransfer.effectAllowed = 'move';
    }
    sidebar.setDrag(node.id);
  };

  const onDragOver = (event: TargetedDragEvent<HTMLDivElement>) => {
    // Rows own their drag events: an invalid drop here must not fall through to the container.
    event.stopPropagation();
    const dragId = state.dragId;
    if (dragId === null) return;
    const zone = zoneFromPointer(event, node.kind);
    if (placementForRow(snapshot, dragId, node.id, zone) === null) {
      sidebar.setDrop(null);
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    sidebar.setDrop({ target: { kind: 'row', id: node.id }, zone });
  };

  const onDrop = (event: TargetedDragEvent<HTMLDivElement>) => {
    event.stopPropagation();
    event.preventDefault();
    const dragId = state.dragId;
    const zone = zoneFromPointer(event, node.kind);
    sidebar.setDrag(null);
    if (dragId !== null) void sidebar.dropOnRow(dragId, node.id, zone);
  };

  return (
    <li
      role="treeitem"
      class={cx('item', isTab && !node.live && 'suspended')}
      tabIndex={ctx.focusId === node.id ? 0 : -1}
      data-node-id={node.id}
      aria-level={level}
      aria-setsize={setSize}
      aria-posinset={index + 1}
      aria-label={ariaLabel}
      aria-expanded={isFolder ? !collapsed : undefined}
      aria-selected={isTab ? selected : undefined}
      ref={(el) => ctx.register(node.id, el)}
    >
      <div
        class={cx('row', selected && 'selected', hint && `drop-${hint}`)}
        draggable={!renaming}
        onClick={onRowClick}
        onDragStart={onDragStart}
        onDragOver={onDragOver}
        onDrop={onDrop}
        onDragEnd={() => sidebar.setDrag(null)}
      >
        {isFolder ? <ChevronIcon open={!collapsed} /> : <span class="twisty" aria-hidden="true" />}
        {isFolder ? <FolderIcon /> : <GlobeIcon />}
        {renaming ? (
          <RenameField
            initial={node.title}
            onDone={(value, refocus) => sidebar.finishRename(node.id, value, refocus)}
          />
        ) : (
          <span class="title" title={node.url ?? undefined}>
            {label}
          </span>
        )}
        {isTab && node.live ? <span class="live-dot" aria-hidden="true" /> : null}
        {isTab && node.is_unread ? <span class="unread-dot" aria-hidden="true" /> : null}
        {isTab && !renaming ? (
          <button
            type="button"
            class="close"
            tabIndex={-1}
            aria-label={`Close ${label}`}
            onClick={(event) => {
              event.stopPropagation();
              ctx.close(node.id);
            }}
          >
            <CloseIcon />
          </button>
        ) : null}
      </div>
      {isFolder && !collapsed && node.children.length > 0 ? (
        <ul class="list" role="group">
          {node.children.map((child, childIndex) => (
            <TreeItem
              key={child.id}
              node={child}
              level={level + 1}
              index={childIndex}
              setSize={node.children.length}
              ctx={ctx}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

function dropHintFor(drop: DropHint | null, id: NodeId): DropZone | null {
  if (drop?.target.kind !== 'row' || drop.target.id !== id) return null;
  return drop.zone;
}

/**
 * Inline title editor. Enter commits and focus returns to the row. Escape cancels and focus returns.
 * Blur commits. Focus returns only when focus went nowhere (`relatedTarget` is null), so a click on
 * another control keeps focus there. Keys stop here so the tree does not act on them.
 */
function RenameField({ initial, onDone }: { initial: string; onDone: (value: string | null, refocus: boolean) => void }) {
  const [value, setValue] = useState(initial);
  const finished = useRef(false);
  const input = useRef<HTMLInputElement>(null);

  useLayoutEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const finish = (commit: boolean, refocus: boolean) => {
    if (finished.current) return;
    finished.current = true;
    onDone(commit ? value : null, refocus);
  };

  return (
    <input
      ref={input}
      type="text"
      class="rename"
      aria-label="Rename"
      value={value}
      onInput={(event) => setValue(event.currentTarget.value)}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === 'Enter') {
          event.preventDefault();
          finish(true, true);
        } else if (event.key === 'Escape') {
          event.preventDefault();
          finish(false, true);
        }
      }}
      onClick={(event) => event.stopPropagation()}
      onBlur={(event) => finish(true, event.relatedTarget === null)}
    />
  );
}
