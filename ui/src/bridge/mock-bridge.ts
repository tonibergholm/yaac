import type {
  ApiResult,
  Body,
  Command,
  ContainerKind,
  CoreEvent,
  ErrorCode,
  NodeId,
  NodeView,
  Placement,
  Query,
  ResponseBody,
  Snapshot,
  SpaceId,
} from '../wire';
import type { Bridge, EventHandler } from './types';

/**
 * In-memory core for `pnpm dev`, tests and screenshots. It mirrors the rules the Rust core
 * enforces: tree shape, no moves into self, the scheme allow-list and the error codes. That is
 * close enough to exercise the UI. It is not a substitute for the core's tests.
 */

const CONTAINERS: readonly ContainerKind[] = ['favorites', 'pinned', 'today', 'archive'];
const WINDOW_ID = 'window-1';

interface MockSpace {
  id: SpaceId;
  name: string;
  containers: Record<ContainerKind, NodeView[]>;
}

interface Found {
  node: NodeView;
  list: NodeView[];
  parent: NodeView | null;
  container: ContainerKind;
  space: MockSpace;
}

class MockFailure extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
  }
}

function fail(code: ErrorCode, message: string): never {
  throw new MockFailure(code, message);
}

function assertNever(value: never): never {
  return fail('bad_request', `unhandled command ${JSON.stringify(value)}`);
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function assertAllowedUrl(url: string): void {
  if (url === 'about:blank') return;
  let protocol: string;
  try {
    protocol = new URL(url).protocol;
  } catch {
    fail('bad_request', 'url is not a valid address');
  }
  if (protocol !== 'http:' && protocol !== 'https:') {
    fail('bad_request', 'only http, https and about:blank are allowed');
  }
}

function containsId(node: NodeView, id: NodeId): boolean {
  if (node.id === id) return true;
  return node.children.some((child) => containsId(child, id));
}

function newNode(
  id: NodeId,
  kind: NodeView['kind'],
  title: string,
  url: string | null,
  props: { live?: boolean; is_unread?: boolean; children?: NodeView[] } = {},
): NodeView {
  return {
    id,
    kind,
    title,
    url,
    is_unread: props.is_unread ?? false,
    live: props.live ?? false,
    children: props.children ?? [],
  };
}

export class MockBridge implements Bridge {
  private rev = 1;
  private nextId = 1;
  private nextSpace = 1;
  private selected: NodeId | null = null;
  private activeSpace: SpaceId = '';
  private spaces: MockSpace[] = [];
  private readonly handlers = new Set<EventHandler>();

  constructor() {
    this.seed();
  }

  request(body: Body): Promise<ApiResult> {
    return Promise.resolve().then(() => this.apply(body));
  }

  onEvent(handler: EventHandler): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  private apply(body: Body): ApiResult {
    try {
      if (body.kind === 'query') {
        return { status: 'ok', body: this.query(body.body) };
      }
      return { status: 'ok', body: this.command(body.body) };
    } catch (error) {
      if (error instanceof MockFailure) {
        return { status: 'err', error: { code: error.code, message: error.message } };
      }
      throw error;
    }
  }

  private query(query: Query): ResponseBody {
    switch (query.type) {
      case 'snapshot':
      case 'since':
        return { type: 'snapshot', ...this.snapshot() };
      default:
        return assertNever(query);
    }
  }

  private command(command: Command): ResponseBody {
    switch (command.type) {
      case 'open_tab': {
        assertAllowedUrl(command.url);
        const { list } = this.resolvePlacement(command.placement);
        const node = newNode(this.id(), 'tab', '', command.url, { live: true });
        this.insert(list, node, command.placement.before);
        if (command.activate) this.selected = node.id;
        return { type: 'opened', node: node.id, rev: this.bump() };
      }
      case 'close_tab': {
        const found = this.requireTab(command.node);
        if (found.container === 'archive') return this.ack();
        const [node] = found.list.splice(found.list.indexOf(found.node), 1);
        if (!node) fail('internal', 'tab vanished during close');
        node.live = false;
        found.space.containers.archive.push(node);
        if (this.selected === node.id) this.selected = this.firstTodayTab();
        return this.ack(this.bump());
      }
      case 'select_tab': {
        const found = this.requireTab(command.node);
        if (found.container === 'archive') fail('bad_request', 'archived tabs cannot be selected');
        found.node.live = true;
        if (this.selected === found.node.id) return this.ack();
        this.selected = found.node.id;
        return this.ack(this.bump());
      }
      case 'move_node': {
        const found = this.find(command.node);
        if (!found) fail('not_found', `node ${command.node} does not exist`);
        if (command.to.before === command.node) {
          fail('bad_request', 'a node cannot be placed before itself');
        }
        if (command.to.parent !== null && containsId(found.node, command.to.parent)) {
          fail('bad_request', 'a node cannot be moved into itself or its descendants');
        }
        const target = this.resolvePlacement(command.to);
        found.list.splice(found.list.indexOf(found.node), 1);
        this.insert(target.list, found.node, command.to.before);
        return this.ack(this.bump());
      }
      case 'create_folder': {
        const title = this.requireTitle(command.title);
        const { list } = this.resolvePlacement(command.placement);
        this.insert(list, newNode(this.id(), 'folder', title, null), command.placement.before);
        return this.ack(this.bump());
      }
      case 'rename_node': {
        const title = this.requireTitle(command.title);
        const found = this.find(command.node);
        if (!found) fail('not_found', `node ${command.node} does not exist`);
        found.node.title = title;
        return this.ack(this.bump());
      }
      case 'create_space': {
        const name = this.requireTitle(command.name);
        const space: MockSpace = {
          id: `space-${this.nextSpace++}`,
          name,
          containers: { favorites: [], pinned: [], today: [], archive: [] },
        };
        this.spaces.push(space);
        return { type: 'space_created', space: space.id, rev: this.bump() };
      }
      case 'navigate': {
        assertAllowedUrl(command.url);
        const found = this.requireTab(command.node);
        found.node.url = command.url;
        found.node.title = '';
        found.node.live = true;
        return this.ack(this.bump());
      }
      case 'back':
      case 'forward':
      case 'reload':
        this.requireTab(command.node);
        return this.ack();
      default:
        return assertNever(command);
    }
  }

  private ack(rev: number = this.rev): ResponseBody {
    return { type: 'ack', rev };
  }

  private bump(): number {
    this.rev += 1;
    const rev = this.rev;
    setTimeout(() => {
      const event: CoreEvent = { topic: 'tree', rev };
      for (const handler of [...this.handlers]) handler(event);
    }, 0);
    return rev;
  }

  private id(): NodeId {
    return `node-${this.nextId++}`;
  }

  private requireTitle(value: string): string {
    const trimmed = value.trim();
    if (trimmed === '') fail('bad_request', 'title must not be empty');
    return trimmed;
  }

  private requireTab(id: NodeId): Found {
    const found = this.find(id);
    if (!found) fail('not_found', `node ${id} does not exist`);
    if (found.node.kind !== 'tab') fail('bad_request', `node ${id} is not a tab`);
    return found;
  }

  private find(id: NodeId): Found | null {
    for (const space of this.spaces) {
      for (const container of CONTAINERS) {
        const hit = this.findIn(space.containers[container], null, id);
        if (hit) return { ...hit, container, space };
      }
    }
    return null;
  }

  private findIn(
    list: NodeView[],
    parent: NodeView | null,
    id: NodeId,
  ): Omit<Found, 'container' | 'space'> | null {
    for (const node of list) {
      if (node.id === id) return { node, list, parent };
      const hit = this.findIn(node.children, node, id);
      if (hit) return hit;
    }
    return null;
  }

  /** Resolves a placement to the sibling list it targets, validating space, container, parent and before. */
  private resolvePlacement(placement: Placement): { list: NodeView[] } {
    const space = this.spaces.find((s) => s.id === placement.space);
    if (!space) fail('not_found', `space ${placement.space} does not exist`);
    let list: NodeView[];
    if (placement.parent === null) {
      list = space.containers[placement.container];
    } else {
      const parent = this.find(placement.parent);
      if (!parent) fail('not_found', `parent ${placement.parent} does not exist`);
      if (parent.node.kind !== 'folder') fail('bad_request', 'parent must be a folder');
      if (parent.space !== space || parent.container !== placement.container) {
        fail('bad_request', 'parent is not in the target container');
      }
      list = parent.node.children;
    }
    if (placement.before !== null && !list.some((n) => n.id === placement.before)) {
      fail('not_found', 'before is not a sibling in the target container');
    }
    return { list };
  }

  private insert(list: NodeView[], node: NodeView, before: NodeId | null): void {
    const index = before === null ? list.length : list.findIndex((n) => n.id === before);
    list.splice(index, 0, node);
  }

  private firstTodayTab(): NodeId | null {
    const space = this.spaces.find((s) => s.id === this.activeSpace);
    return space?.containers.today.find((n) => n.kind === 'tab')?.id ?? null;
  }

  private snapshot(): Snapshot {
    return clone({
      rev: this.rev,
      window: WINDOW_ID,
      active_space: this.activeSpace,
      selected: this.selected,
      spaces: this.spaces.map((space) => ({
        id: space.id,
        name: space.name,
        theme: null,
        containers: CONTAINERS.map((kind) => ({ kind, nodes: space.containers[kind] })),
      })),
    });
  }

  private seed(): void {
    const personal: MockSpace = {
      id: 'space-personal',
      name: 'Personal',
      containers: { favorites: [], pinned: [], today: [], archive: [] },
    };
    this.spaces.push(personal);
    this.activeSpace = personal.id;
    this.nextSpace = 2;

    const design = newNode(this.id(), 'tab', 'Design doc', 'https://docs.example.com/design', {
      live: true,
    });
    const repo = newNode(this.id(), 'tab', '', 'https://github.com/yaac-dev/yaac', {
      is_unread: true,
    });
    const work = newNode(this.id(), 'folder', 'Work', null, { children: [design, repo] });
    personal.containers.pinned.push(
      newNode(this.id(), 'tab', 'Mail', 'https://mail.example.com/'),
      work,
    );

    const example = newNode(this.id(), 'tab', 'Example Domain', 'https://example.org/', {
      live: true,
    });
    const wiki = newNode(this.id(), 'tab', '', 'https://en.wikipedia.org/wiki/Web_browser');
    const settings = newNode(this.id(), 'tab', 'New tab', 'about:blank', { live: true });
    personal.containers.today.push(example, wiki, settings);
    this.selected = example.id;
  }
}
