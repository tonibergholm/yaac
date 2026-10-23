// The single import point for the wire contract. Everything here is generated from yaac-api by
// `pnpm gen`. This file only re-exports names and adds the endpoint constants.

export type { ApiError } from './generated/ApiError';
export type { ApiResult } from './generated/ApiResult';
export type { Body } from './generated/Body';
export type { Command } from './generated/Command';
export type { ContainerKind } from './generated/ContainerKind';
export type { ContainerView } from './generated/ContainerView';
export type { ErrorCode } from './generated/ErrorCode';
export type { Event as CoreEvent } from './generated/Event';
export type { NodeId } from './generated/NodeId';
export type { NodeKind } from './generated/NodeKind';
export type { NodeView } from './generated/NodeView';
export type { Placement } from './generated/Placement';
export type { Query } from './generated/Query';
export type { Request as ApiRequest } from './generated/Request';
export type { Response as ApiResponse } from './generated/Response';
export type { ResponseBody } from './generated/ResponseBody';
export type { Snapshot } from './generated/Snapshot';
export type { SpaceId } from './generated/SpaceId';
export type { SpaceView } from './generated/SpaceView';
export type { Topic } from './generated/Topic';
export type { WindowId } from './generated/WindowId';

/** Must equal `yaac_api::API_VERSION`. */
export const API_VERSION = 1;

/** Served by the `app://ui` scheme handler in the host. */
export const RPC_ENDPOINT = 'app://ui/api/v1/rpc';
