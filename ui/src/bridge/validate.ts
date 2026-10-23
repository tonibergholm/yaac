import type { ApiResult } from '../wire';
import { ProtocolError } from './types';

// Checks a reply from the host before the UI uses it. The types are generated from Rust, so this
// only guards against a host that is out of step with the page. A failed check throws a
// `ProtocolError`. The sidebar treats that as a permanent refusal, since retrying returns the same reply.

type Obj = Record<string, unknown>;

const NODE_KINDS = ['tab', 'folder', 'split'] as const;
const CONTAINER_KINDS = ['favorites', 'pinned', 'today', 'archive'] as const;
const ERROR_CODES = ['bad_request', 'not_found', 'forbidden', 'conflict', 'unsupported_version', 'internal'] as const;

function isObj(value: unknown): value is Obj {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function oneOf(list: readonly string[], value: unknown): boolean {
  return typeof value === 'string' && list.includes(value);
}

function isRev(value: unknown): boolean {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isNode(value: unknown): boolean {
  return (
    isObj(value) &&
    typeof value.id === 'string' &&
    oneOf(NODE_KINDS, value.kind) &&
    typeof value.title === 'string' &&
    (value.url === null || typeof value.url === 'string') &&
    typeof value.is_unread === 'boolean' &&
    typeof value.live === 'boolean' &&
    Array.isArray(value.children) &&
    value.children.every(isNode)
  );
}

function isSpace(value: unknown): boolean {
  return (
    isObj(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    (value.theme === null || typeof value.theme === 'string') &&
    Array.isArray(value.containers) &&
    value.containers.every(
      (c: unknown) => isObj(c) && oneOf(CONTAINER_KINDS, c.kind) && Array.isArray(c.nodes) && c.nodes.every(isNode),
    )
  );
}

function isSnapshot(value: unknown): boolean {
  return (
    isObj(value) &&
    isRev(value.rev) &&
    typeof value.window === 'string' &&
    typeof value.active_space === 'string' &&
    (value.selected === null || typeof value.selected === 'string') &&
    Array.isArray(value.spaces) &&
    value.spaces.every(isSpace)
  );
}

function isResponseBody(value: unknown): boolean {
  if (!isObj(value)) return false;
  switch (value.type) {
    case 'ack':
      return isRev(value.rev);
    case 'opened':
      return typeof value.node === 'string' && isRev(value.rev);
    case 'space_created':
      return typeof value.space === 'string' && isRev(value.rev);
    case 'snapshot':
      return isSnapshot(value);
    default:
      return false;
  }
}

/** Validates the `Response` envelope for request `expectedId` and returns its `ApiResult`. */
export function parseApiResponse(value: unknown, expectedId: number): ApiResult {
  if (!isObj(value) || value.id !== expectedId) {
    throw new ProtocolError('rpc reply does not answer this request');
  }
  const result = value.result;
  if (isObj(result)) {
    if (result.status === 'ok' && isResponseBody(result.body)) return result as unknown as ApiResult;
    if (result.status === 'err' && isObj(result.error) && oneOf(ERROR_CODES, result.error.code)) {
      if (typeof result.error.message === 'string') return result as unknown as ApiResult;
    }
  }
  throw new ProtocolError('rpc reply is malformed');
}
