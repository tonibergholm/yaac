import type { ContainerKind, NodeId, NodeView, Placement, Snapshot, SpaceView } from '../wire';

/** The containers the sidebar shows in M1. Favorites and archive are not rendered. */
export const SIDEBAR_CONTAINERS = ['pinned', 'today'] as const satisfies readonly ContainerKind[];
export type SidebarContainer = (typeof SIDEBAR_CONTAINERS)[number];

export function activeSpace(snapshot: Snapshot): SpaceView | undefined {
  return snapshot.spaces.find((space) => space.id === snapshot.active_space);
}

export function containerNodes(space: SpaceView, kind: ContainerKind): readonly NodeView[] {
  return space.containers.find((c) => c.kind === kind)?.nodes ?? [];
}

export interface Location {
  node: NodeView;
  container: ContainerKind;
  parent: NodeView | null;
  siblings: readonly NodeView[];
  index: number;
}

/** Finds a node in any container of the Space, with its parent and sibling list. */
export function locate(space: SpaceView, id: NodeId): Location | null {
  for (const container of space.containers) {
    const hit = findIn(container.nodes, null, id, container.kind);
    if (hit) return hit;
  }
  return null;
}

function findIn(
  list: readonly NodeView[],
  parent: NodeView | null,
  id: NodeId,
  container: ContainerKind,
): Location | null {
  for (let index = 0; index < list.length; index++) {
    const node = list[index];
    if (!node) continue;
    if (node.id === id) return { node, container, parent, siblings: list, index };
    const inner = findIn(node.children, node, id, container);
    if (inner) return inner;
  }
  return null;
}

export function containsId(node: NodeView, id: NodeId): boolean {
  return node.id === id || node.children.some((child) => containsId(child, id));
}

export interface VisibleRow {
  node: NodeView;
  container: SidebarContainer;
  parent: NodeView | null;
  level: number;
  index: number;
  setSize: number;
}

/** Rows in display order, skipping the children of collapsed folders. Drives keyboard navigation. */
export function visibleRows(space: SpaceView, collapsed: ReadonlySet<NodeId>): VisibleRow[] {
  const rows: VisibleRow[] = [];
  for (const container of SIDEBAR_CONTAINERS) {
    walk(containerNodes(space, container), null, 1, container, collapsed, rows);
  }
  return rows;
}

function walk(
  list: readonly NodeView[],
  parent: NodeView | null,
  level: number,
  container: SidebarContainer,
  collapsed: ReadonlySet<NodeId>,
  out: VisibleRow[],
): void {
  list.forEach((node, index) => {
    out.push({ node, container, parent, level, index, setSize: list.length });
    if (node.kind === 'folder' && !collapsed.has(node.id)) {
      walk(node.children, node, level + 1, container, collapsed, out);
    }
  });
}

/** Host name of an address, or null when it has none. That covers about:blank and malformed input. */
export function hostnameOf(url: string | null): string | null {
  if (!url) return null;
  try {
    return new URL(url).hostname || null;
  } catch {
    return null;
  }
}

/** Title shown in the tree. Titles are untrusted: callers render the result as text only. */
export function displayTitle(node: NodeView): string {
  const title = node.title.trim();
  if (title !== '') return title;
  if (node.kind === 'folder') return 'Untitled folder';
  return hostnameOf(node.url) ?? (node.url?.trim() || 'New tab');
}

export type DropTarget = { kind: 'row'; id: NodeId } | { kind: 'container'; container: SidebarContainer };
export type DropZone = 'before' | 'after' | 'inside';

/** Zone under the pointer. Folders have a middle band that means "inside"; other rows split at the midpoint. */
export function zoneForRatio(kind: NodeView['kind'], ratio: number): DropZone {
  if (kind === 'folder') {
    if (ratio < 0.25) return 'before';
    if (ratio > 0.75) return 'after';
    return 'inside';
  }
  return ratio < 0.5 ? 'before' : 'after';
}

/**
 * Placement for dropping `draggedId` on a row. Returns null when the drop is invalid or a no-op:
 * onto itself, into its own subtree, into a split, or inside a non-folder. Splits hold tabs only.
 * It also returns null for the position the node already occupies.
 */
export function placementForRow(
  snapshot: Snapshot,
  draggedId: NodeId,
  targetId: NodeId,
  zone: DropZone,
): Placement | null {
  const space = activeSpace(snapshot);
  if (!space || draggedId === targetId) return null;
  const dragged = locate(space, draggedId);
  const target = locate(space, targetId);
  if (!dragged || !target) return null;
  if (containsId(dragged.node, targetId)) return null;
  if (target.parent?.kind === 'split') return null;

  const base = { space: space.id, container: target.container, parent: target.parent?.id ?? null };
  if (zone === 'inside') {
    if (target.node.kind !== 'folder') return null;
    return { ...base, parent: target.node.id, before: null };
  }
  if (zone === 'before') {
    return { ...base, before: target.node.id };
  }
  const next = target.siblings[target.index + 1];
  if (next?.id === draggedId) return null;
  return { ...base, before: next?.id ?? null };
}

/** Placement for dropping `draggedId` at the end of a container's root list. */
export function placementAtContainerEnd(
  snapshot: Snapshot,
  draggedId: NodeId,
  container: SidebarContainer,
): Placement | null {
  const space = activeSpace(snapshot);
  if (!space || !locate(space, draggedId)) return null;
  return { space: space.id, container, parent: null, before: null };
}

/** Placement that moves a node one step up or down among its siblings. It is the keyboard alternative to dragging. */
export function placementForSiblingStep(
  snapshot: Snapshot,
  id: NodeId,
  direction: -1 | 1,
): Placement | null {
  const space = activeSpace(snapshot);
  if (!space) return null;
  const loc = locate(space, id);
  if (!loc) return null;
  const base = { space: space.id, container: loc.container, parent: loc.parent?.id ?? null };
  if (direction === -1) {
    const previous = loc.siblings[loc.index - 1];
    if (!previous) return null;
    return { ...base, before: previous.id };
  }
  const after = loc.siblings[loc.index + 2];
  if (!loc.siblings[loc.index + 1]) return null;
  return { ...base, before: after?.id ?? null };
}
