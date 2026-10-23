import { RPC_TIMEOUT_MS, RpcTimeoutError, type Bridge } from '../bridge/types';
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
import type { ApiResult, Body, Command, NodeId, Placement, Query, Snapshot, SpaceId } from '../wire';
import { Store } from './store';

export const BACKOFF_MIN_MS = 250;
export const BACKOFF_MAX_MS = 5_000;

const UNREACHABLE = 'Cannot reach the browser core.';
const COMMAND_TIMEOUT = 'The browser core did not answer in time. Check the tree, then try again.';
const UNEXPECTED = 'The browser core sent an unexpected response.';

export interface DropHint {
  target: DropTarget;
  zone: DropZone;
}

/** Latest core state plus UI-only state. The snapshot is replaced wholesale, never patched. */
export interface UiState {
  snapshot: Snapshot | null;
  load: 'loading' | 'ready' | 'failed';
  /** `reconnecting` while pulls fail. The tree keeps showing the last known state. */
  link: 'ok' | 'reconnecting';
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

function assertNever(value: never): never {
  throw new Error(`unhandled response ${JSON.stringify(value)}`);
}

/**
 * Owns sync with the core and every action the UI can take.
 *
 * Sync: events carry only a revision. Bursts of events collapse into one `since` pull. A pull
 * runs alone, and events that arrive meanwhile queue one follow-up pull. A snapshot with a lower
 * revision than the applied one is dropped.
 *
 * Failure: every request has a timeout. A failed or timed-out pull retries with exponential
 * backoff (250 ms, doubling, capped at 5 s) until a snapshot arrives. A new event cancels the
 * backoff and pulls at once. A command that times out reports an error and triggers a pull.
 */
export class Sidebar {
  readonly store = new Store<UiState>({
    snapshot: null,
    load: 'loading',
    link: 'ok',
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
  private pullScheduled = false;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private backoffMs = BACKOFF_MIN_MS;
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
    this.clearRetry();
  }

  /** Manual retry, for example from the Retry button. Resets the backoff. */
  retry(): void {
    this.backoffMs = BACKOFF_MIN_MS;
    this.store.update({ load: 'loading', notice: null });
    this.resync();
  }

  // ---- transport with timeout -------------------------------------------

  /** Sends a request and rejects with `RpcTimeoutError` when no reply arrives in time. */
  private request(body: Body): Promise<ApiResult> {
    return new Promise<ApiResult>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new RpcTimeoutError(`no answer within ${RPC_TIMEOUT_MS} ms`)),
        RPC_TIMEOUT_MS,
      );
      let pending: Promise<ApiResult>;
      try {
        pending = this.bridge.request(body);
      } catch (error) {
        clearTimeout(timer);
        reject(error);
        return;
      }
      pending.then(
        (result) => {
          clearTimeout(timer);
          resolve(result);
        },
        (error: unknown) => {
          clearTimeout(timer);
          reject(error);
        },
      );
    });
  }

  // ---- sync -------------------------------------------------------------

  private appliedRev(): number {
    return this.store.get().snapshot?.rev ?? -1;
  }

  private needsPull(): boolean {
    return this.store.get().snapshot === null || this.latestRev > this.appliedRev() || this.resyncRequested;
  }

  private noteRev(rev: number): void {
    if (rev > this.latestRev) this.latestRev = rev;
    if (rev > this.appliedRev()) this.schedulePull();
  }

  private resync(): void {
    this.resyncRequested = true;
    this.schedulePull();
  }

  /**
   * Coalesces every request made in the same task into one pull. A microtask, not a timer, so the
   * pull runs without waiting on the clock.
   */
  private schedulePull(): void {
    if (this.pullScheduled) return;
    this.pullScheduled = true;
    queueMicrotask(() => {
      this.pullScheduled = false;
      this.run();
    });
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
    // An explicit pull supersedes any scheduled retry.
    this.clearRetry();
    const current = this.store.get().snapshot;
    const query: Query = current ? { type: 'since', rev: current.rev } : { type: 'snapshot' };
    let succeeded: boolean;
    try {
      succeeded = this.handleQuery(await this.request({ kind: 'query', body: query }));
    } catch {
      succeeded = false;
    } finally {
      this.inFlight = false;
    }
    if (succeeded) {
      this.backoffMs = BACKOFF_MIN_MS;
    } else {
      this.scheduleRetry();
    }
    if (this.queued) {
      this.queued = false;
      this.schedulePull();
    }
  }

  /** Returns true when the reply was usable. */
  private handleQuery(result: ApiResult): boolean {
    if (result.status === 'err') {
      this.store.update({ notice: result.error.message });
      return false;
    }
    const body = result.body;
    switch (body.type) {
      case 'snapshot':
        this.applySnapshot(body);
        return true;
      case 'ack':
      case 'opened':
      case 'space_created':
        this.store.update({ notice: UNEXPECTED });
        return false;
      default:
        return assertNever(body);
    }
  }

  private applySnapshot(snapshot: Snapshot): void {
    // Any usable reply proves the link is up, even a stale one.
    this.store.update({ link: 'ok' });
    if (snapshot.rev < this.appliedRev()) return; // stale: a newer state is already applied
    this.store.update({ snapshot, load: 'ready' });
  }

  private scheduleRetry(): void {
    const loaded = this.store.get().snapshot !== null;
    this.store.update({ link: 'reconnecting', load: loaded ? 'ready' : 'failed' });
    this.resyncRequested = true;
    const delay = this.backoffMs;
    this.backoffMs = Math.min(this.backoffMs * 2, BACKOFF_MAX_MS);
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.schedulePull();
    }, delay);
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }

  // ---- commands ---------------------------------------------------------

  private async send(body: Command): Promise<ApiResult | null> {
    let result: ApiResult;
    try {
      result = await this.request({ kind: 'command', body });
    } catch (error) {
      this.store.update({ notice: error instanceof RpcTimeoutError ? COMMAND_TIMEOUT : UNREACHABLE });
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

  private spaceId(): SpaceId | null {
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

  /** Switches the window to another Space. Selecting the active Space sends nothing. */
  async selectSpace(space: SpaceId): Promise<ApiResult | null> {
    if (space === this.spaceId()) return null;
    return this.send({ type: 'select_space', space });
  }

  /** Creates a folder at the root of `container`, then starts an inline rename on it. */
  async createFolder(container: SidebarContainer, title: string): Promise<ApiResult | null> {
    const trimmed = title.trim();
    const space = this.spaceId();
    if (trimmed === '' || space === null) return null;
    const result = await this.send({
      type: 'create_folder',
      placement: { space, container, parent: null, before: null },
      title: trimmed,
    });
    if (result?.status === 'ok' && result.body.type === 'opened') {
      this.store.update({ renamingId: result.body.node });
    }
    return result;
  }

  async renameNode(node: NodeId, title: string): Promise<ApiResult | null> {
    const trimmed = title.trim();
    if (trimmed === '') return null;
    return this.send({ type: 'rename_node', node, title: trimmed });
  }

  async moveNode(node: NodeId, to: Placement): Promise<ApiResult | null> {
    return this.send({ type: 'move_node', node, to });
  }

  /** Creates a Space, then switches to it. */
  async createSpace(name: string): Promise<ApiResult | null> {
    const trimmed = name.trim();
    if (trimmed === '') return null;
    const result = await this.send({ type: 'create_space', name: trimmed });
    if (result?.status === 'ok' && result.body.type === 'space_created') {
      await this.selectSpace(result.body.space);
    }
    return result;
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

  /**
   * Ends an inline rename. A null or unchanged title sends nothing. `refocus` moves focus back
   * to the row; it is false when the user clicked somewhere else.
   */
  finishRename(id: NodeId, title: string | null, refocus = true): void {
    this.store.update({ renamingId: null });
    if (refocus) this.setFocus(id, true);
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
