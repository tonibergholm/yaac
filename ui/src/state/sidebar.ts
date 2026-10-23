import type { Bridge } from '../bridge/types';
import { normalizeAddress } from '../model/address';
import {
  activeSpace,
  locate,
  placementAtContainerEnd,
  placementForRow,
  placementForSiblingStep,
  type DropTarget,
  type DropZone,
  type SidebarContainer,
} from '../model/tree';
import type { ApiResult, Command, NodeId, Placement, Query, Snapshot } from '../wire';
import { Store } from './store';

export interface DropHint {
  target: DropTarget;
  zone: DropZone;
}

/** Latest core state plus UI-only state. The snapshot is replaced wholesale, never patched. */
export interface UiState {
  snapshot: Snapshot | null;
  load: 'loading' | 'ready' | 'failed';
  notice: string | null;
  collapsed: ReadonlySet<NodeId>;
  renamingId: NodeId | null;
  focusId: NodeId | null;
  focusRequested: boolean;
  /** Focus waits until the rendered snapshot reaches this revision. A move reorders the DOM. */
  focusRev: number;
  dragId: NodeId | null;
  drop: DropHint | null;
}

const UNREACHABLE = 'Cannot reach the browser core.';
const UNEXPECTED = 'The browser core sent an unexpected response.';

/**
 * Owns sync with the core and every action the UI can take.
 *
 * Sync: events carry only a revision. Bursts of events collapse into one `since` pull. A pull
 * runs alone; events that arrive meanwhile queue one follow-up pull. A snapshot with a lower
 * revision than the applied one is dropped.
 */
export class Sidebar {
  readonly store = new Store<UiState>({
    snapshot: null,
    load: 'loading',
    notice: null,
    collapsed: new Set(),
    renamingId: null,
    focusId: null,
    focusRequested: false,
    focusRev: 0,
    dragId: null,
    drop: null,
  });

  private unsubscribe: (() => void) | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private inFlight = false;
  private queued = false;
  private resyncRequested = false;
  /** Highest revision seen in an event or an acknowledged command. */
  private latestRev = -1;

  constructor(private readonly bridge: Bridge) {}

  start(): void {
    if (this.unsubscribe) return;
    this.unsubscribe = this.bridge.onEvent((event) => this.noteRev(event.rev));
    this.schedulePull();
  }

  stop(): void {
    this.unsubscribe?.();
    this.unsubscribe = null;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  retry(): void {
    this.store.update({ load: 'loading', notice: null });
    this.resync();
  }

  // ---- sync -------------------------------------------------------------

  private appliedRev(): number {
    return this.store.get().snapshot?.rev ?? -1;
  }

  private needsPull(): boolean {
    return (
      this.store.get().snapshot === null ||
      this.latestRev > this.appliedRev() ||
      this.resyncRequested
    );
  }

  private noteRev(rev: number): void {
    if (rev > this.latestRev) this.latestRev = rev;
    if (rev > this.appliedRev()) this.schedulePull();
  }

  private resync(): void {
    this.resyncRequested = true;
    this.schedulePull();
  }

  /** Coalesces every request made in the same tick into one pull. */
  private schedulePull(): void {
    if (this.timer !== null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.run();
    }, 0);
  }

  private run(): void {
    if (this.inFlight) {
      this.queued = true;
      return;
    }
    if (!this.needsPull()) return;
    void this.pull();
  }

  private async pull(): Promise<void> {
    this.inFlight = true;
    this.resyncRequested = false;
    const current = this.store.get().snapshot;
    const query: Query = current ? { type: 'since', rev: current.rev } : { type: 'snapshot' };
    try {
      const result = await this.bridge.request({ kind: 'query', body: query });
      if (result.status === 'err') {
        this.fail(result.error.message);
      } else if (result.body.type !== 'snapshot') {
        this.fail(UNEXPECTED);
      } else {
        this.applySnapshot(result.body);
      }
    } catch {
      this.fail(UNREACHABLE);
    } finally {
      this.inFlight = false;
      if (this.queued) {
        this.queued = false;
        this.schedulePull();
      }
    }
  }

  private applySnapshot(snapshot: Snapshot): void {
    if (snapshot.rev < this.appliedRev()) return; // stale response: a newer state is already applied
    this.store.update({ snapshot, load: 'ready' });
  }

  private fail(message: string): void {
    // Do not retry on every later event until a newer revision arrives.
    this.latestRev = this.appliedRev();
    const loaded = this.store.get().snapshot !== null;
    this.store.update(loaded ? { notice: message } : { load: 'failed', notice: message });
  }

  // ---- commands ---------------------------------------------------------

  private async send(body: Command): Promise<ApiResult | null> {
    let result: ApiResult;
    try {
      result = await this.bridge.request({ kind: 'command', body });
    } catch {
      this.store.update({ notice: UNREACHABLE });
      this.resync();
      return null;
    }
    if (result.status === 'ok') {
      this.store.update({ notice: null });
      this.noteRev(result.body.rev);
    } else {
      this.store.update({ notice: result.error.message });
      this.resync();
    }
    return result;
  }

  private spaceId(): string | null {
    const snapshot = this.store.get().snapshot;
    return snapshot ? (activeSpace(snapshot)?.id ?? null) : null;
  }

  /** Opens a blank tab at the end of the active Space's `today` container and selects it. */
  async newTab(): Promise<ApiResult | null> {
    const space = this.spaceId();
    if (space === null) return null;
    return this.send({
      type: 'open_tab',
      placement: { space, container: 'today', parent: null, before: null },
      url: 'about:blank',
      activate: true,
    });
  }

  async selectTab(node: NodeId): Promise<ApiResult | null> {
    return this.send({ type: 'select_tab', node });
  }

  async closeTab(node: NodeId): Promise<ApiResult | null> {
    return this.send({ type: 'close_tab', node });
  }

  async createFolder(container: SidebarContainer, title: string): Promise<ApiResult | null> {
    const trimmed = title.trim();
    const space = this.spaceId();
    if (trimmed === '' || space === null) return null;
    return this.send({
      type: 'create_folder',
      placement: { space, container, parent: null, before: null },
      title: trimmed,
    });
  }

  async renameNode(node: NodeId, title: string): Promise<ApiResult | null> {
    const trimmed = title.trim();
    if (trimmed === '') return null;
    return this.send({ type: 'rename_node', node, title: trimmed });
  }

  async moveNode(node: NodeId, to: Placement): Promise<ApiResult | null> {
    return this.send({ type: 'move_node', node, to });
  }

  async createSpace(name: string): Promise<ApiResult | null> {
    const trimmed = name.trim();
    if (trimmed === '') return null;
    return this.send({ type: 'create_space', name: trimmed });
  }

  async navigate(node: NodeId, url: string): Promise<ApiResult | null> {
    const address = normalizeAddress(url);
    if (!address.ok) {
      this.store.update({ notice: address.error });
      return null;
    }
    return this.send({ type: 'navigate', node, url: address.url });
  }

  /** Keyboard alternative to dragging: moves the node one step among its siblings. */
  async moveStep(node: NodeId, direction: -1 | 1): Promise<ApiResult | null> {
    const snapshot = this.store.get().snapshot;
    const placement = snapshot ? placementForSiblingStep(snapshot, node, direction) : null;
    if (!placement) return null;
    return this.moveNode(node, placement);
  }

  /** Commits a drop on a row. Returns the placement sent, or null when the drop is invalid. */
  async dropOnRow(dragId: NodeId, targetId: NodeId, zone: DropZone): Promise<ApiResult | null> {
    const snapshot = this.store.get().snapshot;
    const placement = snapshot ? placementForRow(snapshot, dragId, targetId, zone) : null;
    if (!placement) return null;
    return this.moveNode(dragId, placement);
  }

  /** Commits a drop at the end of a container's root list. */
  async dropAtContainerEnd(dragId: NodeId, container: SidebarContainer): Promise<ApiResult | null> {
    const snapshot = this.store.get().snapshot;
    const placement = snapshot ? placementAtContainerEnd(snapshot, dragId, container) : null;
    if (!placement) return null;
    return this.moveNode(dragId, placement);
  }

  // ---- UI-only state ----------------------------------------------------

  toggleFolder(id: NodeId): void {
    const next = new Set(this.store.get().collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.store.update({ collapsed: next });
  }

  setFocus(id: NodeId | null, requested = false, minRev = 0): void {
    this.store.update({ focusId: id, focusRequested: requested && id !== null, focusRev: minRev });
  }

  clearFocusRequest(): void {
    this.store.update({ focusRequested: false });
  }

  startRename(id: NodeId | null): void {
    this.store.update({ renamingId: id });
  }

  /** Ends an inline rename. A null or unchanged title sends nothing. */
  finishRename(id: NodeId, title: string | null): void {
    this.store.update({ renamingId: null });
    if (title === null) return;
    const snapshot = this.store.get().snapshot;
    const space = snapshot ? activeSpace(snapshot) : undefined;
    const current = space ? locate(space, id)?.node.title : undefined;
    if (current === undefined || title.trim() === current) return;
    void this.renameNode(id, title);
  }

  setDrag(id: NodeId | null): void {
    this.store.update({ dragId: id, drop: null });
  }

  setDrop(hint: DropHint | null): void {
    const current = this.store.get().drop;
    if (JSON.stringify(current) === JSON.stringify(hint)) return;
    this.store.update({ drop: hint });
  }

  dismissNotice(): void {
    this.store.update({ notice: null });
  }
}
