import type { Bridge, EventHandler } from '../src/bridge/types';
import type { ApiResult, Body, Command, CoreEvent, NodeKind, NodeView, Snapshot } from '../src/wire';

export function tab(
  id: string,
  title: string,
  opts: { url?: string | null; live?: boolean; unread?: boolean } = {},
): NodeView {
  return {
    id,
    kind: 'tab',
    title,
    url: opts.url === undefined ? `https://${id}.example.com/` : opts.url,
    is_unread: opts.unread ?? false,
    live: opts.live ?? false,
    children: [],
  };
}

export function folder(id: string, title: string, children: NodeView[] = []): NodeView {
  return { id, kind: 'folder', title, url: null, is_unread: false, live: false, children };
}

export function split(id: string, children: NodeView[]): NodeView {
  return { id, kind: 'split' as NodeKind, title: '', url: null, is_unread: false, live: false, children };
}

export interface SnapshotOptions {
  rev?: number;
  selected?: string | null;
  pinned?: NodeView[];
  today?: NodeView[];
  archive?: NodeView[];
  spaces?: { id: string; name: string }[];
  activeSpace?: string;
}

export function snapshot(options: SnapshotOptions = {}): Snapshot {
  const spaces = options.spaces ?? [{ id: 'space-1', name: 'Personal' }];
  const activeSpace = options.activeSpace ?? spaces[0]?.id ?? 'space-1';
  return {
    rev: options.rev ?? 10,
    window: 'window-1',
    active_space: activeSpace,
    selected: options.selected ?? null,
    spaces: spaces.map((space) => ({
      id: space.id,
      name: space.name,
      theme: null,
      containers: [
        { kind: 'favorites', nodes: [] },
        { kind: 'pinned', nodes: space.id === activeSpace ? (options.pinned ?? []) : [] },
        { kind: 'today', nodes: space.id === activeSpace ? (options.today ?? []) : [] },
        { kind: 'archive', nodes: space.id === activeSpace ? (options.archive ?? []) : [] },
      ],
    })),
  };
}

/** Deep copy so a test can mutate its copy without affecting the source fixture. */
export function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/**
 * Bridge double. Records every request; answers queries from `snap`; acks commands with a
 * revision one above `snap.rev` unless `respond` is replaced.
 */
export class FakeBridge implements Bridge {
  readonly requests: Body[] = [];
  snap: Snapshot;
  respond: (body: Body) => ApiResult | Promise<ApiResult>;
  private readonly handlers = new Set<EventHandler>();

  constructor(snap: Snapshot) {
    this.snap = snap;
    this.respond = (body) => this.defaultRespond(body);
  }

  request(body: Body): Promise<ApiResult> {
    this.requests.push(body);
    return Promise.resolve().then(() => this.respond(body));
  }

  onEvent(handler: EventHandler): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  emit(event: CoreEvent): void {
    for (const handler of [...this.handlers]) handler(event);
  }

  commands(): Command[] {
    return this.requests.flatMap((body) => (body.kind === 'command' ? [body.body] : []));
  }

  queries(): Body[] {
    return this.requests.filter((body) => body.kind === 'query');
  }

  private defaultRespond(body: Body): ApiResult {
    if (body.kind === 'query') {
      return { status: 'ok', body: { type: 'snapshot', ...clone(this.snap) } };
    }
    return { status: 'ok', body: { type: 'ack', rev: this.snap.rev + 1 } };
  }
}

export function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}
