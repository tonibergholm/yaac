import {
  BridgeError,
  ProtocolError,
  RPC_TIMEOUT_MS,
  RpcStatusError,
  RpcTimeoutError,
  type Bridge,
} from '../bridge/types';
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

/**
 * Why the page stopped talking to the core for good. A refusal is permanent: the sidebar stops
 * retrying and offers a manual Retry.
 */
export type Refusal = 'version' | 'forbidden' | 'request' | 'reply';

export function refusalText(refusal: Refusal): string {
  switch (refusal) {
    case 'version':
      return 'The browser core refused this UI version. Update the app, then retry.';
    case 'forbidden':
      return 'The browser core does not allow this page to talk to it. Restart the browser, then retry.';
    case 'request':
      return 'The browser core rejected a request from this UI. Retry, or restart the browser if it persists.';
    case 'reply':
      return 'The browser core sent a reply this UI cannot read. Update the app or the browser, then retry.';
  }
}

/** Permanent refusal for an error code in a reply. Other codes are transient. */
export function refusalOfCode(code: string): Refusal | null {
  switch (code) {
    case 'unsupported_version':
      return 'version';
    case 'forbidden':
      return 'forbidden';
    case 'bad_request':
      return 'request';
    default:
      return null;
  }
}

/**
 * Permanent refusal for a failed request. A timeout, a transport failure and a 5xx status are
 * transient. A reply the UI cannot read, and a 4xx status, are permanent.
 */
export function refusalOfError(error: unknown): Refusal | null {
  if (error instanceof RpcTimeoutError) return null;
  if (error instanceof RpcStatusError) {
    if (error.status === 403) return 'forbidden';
    if (error.status >= 400 && error.status < 500) return 'request';
    return null;
  }
  if (error instanceof ProtocolError) return 'reply';
  if (error instanceof BridgeError) return null;
  // Any other failure is treated as a network failure of unknown kind, so it is retried.
  return null;
}

/**
 * Command replies can refuse the session (version, forbidden, reply). A `bad_request` or a 4xx
 * status on a command is a problem with that one action. It shows as a notice and does not block the
 * tree.
 */
function sessionRefusal(refusal: Refusal | null): Refusal | null {
  return refusal === 'request' ? null : refusal;
}

export interface DropHint {
  target: DropTarget;
  zone: DropZone;
}

/** Latest core state plus UI-only state. The snapshot is replaced wholesale, never patched. */
export interface UiState {
  snapshot: Snapshot | null;
  load: 'loading' | 'ready' | 'failed';
  /**
   * `ok`, `reconnecting` (transient failures, retrying with backoff), or `refused` (a permanent
   * refusal, no retries until a manual Retry).
   */
  link: 'ok' | 'reconnecting' | 'refused';
  refusal: Refusal | null;
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

type Outcome = { ok: true } | { ok: false; refusal: Refusal | null };

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
 * Failure: every request has a timeout. A transient failure (timeout, transport, 5xx, `internal`)
 * retries with exponential backoff (250 ms, doubling, capped at 5 s) until a snapshot arrives. A
 * new event cancels the backoff and pulls at once. A permanent refusal (`unsupported_version`,
 * `forbidden`, `bad_request`, a reply the validator rejects) stops every retry until a manual
 * `retry()`.
 *
 * `stop()` ends all pending work. Nothing is requested after it, until `start()` is called again.
 */
export class Sidebar {
  readonly store = new Store<UiState>({
    snapshot: null,
    load: 'loading',
    link: 'ok',
    refusal: null,
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
  private stopped = false;
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
    this.stopped = false;
    if (this.unsubscribe) return;
    this.unsubscribe = this.bridge.onEvent((event) => this.noteRev(event.rev));
    // Always pull on start: events may have been missed while the sidebar was stopped.
    this.resync();
  }

  /** Ends all pending work: subscriptions, scheduled pulls and retries. Nothing is sent afterwards. */
  stop(): void {
    this.stopped = true;
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.clearRetry();
    this.queued = false;
  }

  /** Manual retry, for example from the Retry button. Clears a refusal and resets the backoff. */
  retry(): void {
    if (this.stopped) return;
    this.backoffMs = BACKOFF_MIN_MS;
    this.store.update({ load: 'loading', notice: null, link: 'ok', refusal: null });
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
    if (this.stopped) return;
    if (rev > this.latestRev) this.latestRev = rev;
    // A refused session waits for a manual retry.
    if (this.store.get().link === 'refused') return;
    if (rev > this.appliedRev()) this.schedulePull();
  }

  private resync(): void {
    if (this.stopped) return;
    this.resyncRequested = true;
    this.schedulePull();
  }

  /**
   * Coalesces every request made in the same task into one pull. A microtask, not a timer, so the
   * pull runs without waiting on the clock.
   */
  private schedulePull(): void {
    if (this.stopped || this.pullScheduled) return;
    this.pullScheduled = true;
    queueMicrotask(() => {
      this.pullScheduled = false;
      this.run();
    });
  }

  private run(): void {
    if (this.stopped) return;
    if (this.inFlight) {
      this.queued = true;
      return;
    }
    if (this.store.get().link === 'refused' && !this.resyncRequested) return;
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
    // A reply that arrives after stop() is dropped, and so is any retry it would have scheduled.
    let outcome: Outcome | null;
    try {
      const reply = await this.request({ kind: 'query', body: query });
      outcome = this.stopped ? null : this.handleQuery(reply);
    } catch (error) {
      outcome = this.stopped ? null : { ok: false, refusal: refusalOfError(error) };
    } finally {
      this.inFlight = false;
    }
    if (outcome === null || this.stopped) return;
    if (outcome.ok) {
      this.backoffMs = BACKOFF_MIN_MS;
    } else if (outcome.refusal !== null) {
      this.refuse(outcome.refusal);
    } else {
      this.scheduleRetry();
    }
    if (this.queued) {
      this.queued = false;
      this.schedulePull();
    }
  }

  private handleQuery(result: ApiResult): Outcome {
    if (result.status === 'err') return { ok: false, refusal: refusalOfCode(result.error.code) };
    const body = result.body;
    switch (body.type) {
      case 'snapshot':
        this.applySnapshot(body);
        return { ok: true };
      case 'ack':
      case 'opened':
      case 'space_created':
        // A query must answer with a snapshot. Any other reply means the core and this UI disagree.
        return { ok: false, refusal: 'reply' };
      default:
        return assertNever(body);
    }
  }

  private applySnapshot(snapshot: Snapshot): void {
    // A usable reply clears the link state. Command notices stay until dismissed or until the next
    // successful command, so a timeout message is not wiped by the fresh pull it triggered.
    this.store.update({ link: 'ok', refusal: null });
    if (snapshot.rev < this.appliedRev()) return; // stale: a newer state is already applied
    this.store.update({ snapshot, load: 'ready' });
  }

  private scheduleRetry(): void {
    if (this.stopped) return;
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

  private refuse(refusal: Refusal): void {
    this.clearRetry();
    this.resyncRequested = false;
    const loaded = this.store.get().snapshot !== null;
    this.store.update({ link: 'refused', refusal, load: loaded ? 'ready' : 'failed' });
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }

  // ---- commands ---------------------------------------------------------

  private async send(body: Command): Promise<ApiResult | null> {
    if (this.stopped) return null;
    let result: ApiResult;
    try {
      result = await this.request({ kind: 'command', body });
    } catch (error) {
      const refusal = sessionRefusal(refusalOfError(error));
      if (refusal !== null) {
        this.refuse(refusal);
        return null;
      }
      this.store.update({ notice: error instanceof RpcTimeoutError ? COMMAND_TIMEOUT : UNREACHABLE });
      this.resync();
      return null;
    }
    if (result.status === 'ok') {
      this.store.update({ notice: null });
      this.noteRev(result.body.rev);
      return result;
    }
    const refusal = sessionRefusal(refusalOfCode(result.error.code));
    if (refusal !== null) {
      this.refuse(refusal);
      return result;
    }
    this.store.update({ notice: result.error.message });
    this.resync();
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

  /**
   * Moves a node. A move into a folder expands that folder first, so the moved row stays visible
   * and keeps focus.
   */
  async moveNode(node: NodeId, to: Placement): Promise<ApiResult | null> {
    if (to.parent !== null) this.expandFolder(to.parent);
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

  expandFolder(id: NodeId): void {
    const current = this.store.get().collapsed;
    if (!current.has(id)) return;
    const next = new Set(current);
    next.delete(id);
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
