import { describe, expect, it } from 'vitest';
import { normalizeAddress } from '../src/model/address';
import {
  displayTitle,
  hostnameOf,
  placementAtContainerEnd,
  placementForRow,
  placementForSiblingStep,
  placementIntoPreviousFolder,
  placementOutOfFolder,
  visibleRows,
  zoneForRatio,
} from '../src/model/tree';
import { folder, snapshot, split, tab } from './support';

describe('displayTitle', () => {
  it('uses the title when present and trims it', () => {
    expect(displayTitle(tab('a', '  Mail  '))).toBe('Mail');
  });

  it('falls back to the hostname when the title is empty', () => {
    expect(displayTitle(tab('a', '', { url: 'https://news.example.org/x?y=1' }))).toBe('news.example.org');
  });

  it('falls back to the raw address when there is no hostname', () => {
    expect(displayTitle(tab('a', '', { url: 'about:blank' }))).toBe('about:blank');
    expect(displayTitle(tab('a', '', { url: null }))).toBe('New tab');
  });

  it('does not throw on a malformed address', () => {
    expect(hostnameOf('not a url')).toBeNull();
    expect(displayTitle(tab('a', '', { url: 'not a url' }))).toBe('not a url');
  });
});

describe('visibleRows', () => {
  it('lists pinned then today and skips children of collapsed folders', () => {
    const snap = snapshot({
      pinned: [folder('f', 'Work', [tab('c1', 'One'), tab('c2', 'Two')])],
      today: [tab('t', 'Today tab')],
    });
    const space = snap.spaces[0]!;
    const open = visibleRows(space, new Set()).map((r) => [r.node.id, r.level]);
    expect(open).toEqual([
      ['f', 1],
      ['c1', 2],
      ['c2', 2],
      ['t', 1],
    ]);
    const closed = visibleRows(space, new Set(['f'])).map((r) => r.node.id);
    expect(closed).toEqual(['f', 't']);
  });
});

describe('zoneForRatio', () => {
  it('splits tabs at the midpoint and gives folders an inside band', () => {
    expect(zoneForRatio('tab', 0.49)).toBe('before');
    expect(zoneForRatio('tab', 0.5)).toBe('after');
    expect(zoneForRatio('folder', 0.1)).toBe('before');
    expect(zoneForRatio('folder', 0.5)).toBe('inside');
    expect(zoneForRatio('folder', 0.9)).toBe('after');
  });
});

describe('placementForRow', () => {
  const build = () =>
    snapshot({
      selected: 'a',
      pinned: [tab('a', 'A'), folder('f', 'Folder', [tab('c', 'Child')]), tab('b', 'B')],
      today: [tab('t', 'T')],
    });

  it('places before a sibling in the same parent and container', () => {
    expect(placementForRow(build(), 't', 'b', 'before')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: null,
      before: 'b',
    });
  });

  it('places after a sibling as before the next sibling, or appends after the last one', () => {
    expect(placementForRow(build(), 't', 'a', 'after')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: null,
      before: 'f',
    });
    expect(placementForRow(build(), 't', 'b', 'after')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: null,
      before: null,
    });
  });

  it('appends into a folder for the inside zone', () => {
    expect(placementForRow(build(), 't', 'f', 'inside')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: 'f',
      before: null,
    });
  });

  it('refuses inside on a tab', () => {
    expect(placementForRow(build(), 't', 'a', 'inside')).toBeNull();
  });

  it('refuses drops onto itself, into its own subtree, and no-op positions', () => {
    expect(placementForRow(build(), 'a', 'a', 'before')).toBeNull();
    expect(placementForRow(build(), 'f', 'c', 'before')).toBeNull();
    expect(placementForRow(build(), 'f', 'f', 'inside')).toBeNull();
    // `a` is directly before `f`; dropping `a` after `a`'s next sibling `f` is a no-op.
    expect(placementForRow(build(), 'f', 'a', 'after')).toBeNull();
  });

  it('refuses drops into a split', () => {
    const snap = snapshot({ pinned: [split('s', [tab('x', 'X'), tab('y', 'Y')])], today: [tab('t', 'T')] });
    expect(placementForRow(snap, 't', 'x', 'before')).toBeNull();
  });

  it('places at the end of a container', () => {
    expect(placementAtContainerEnd(build(), 't', 'pinned')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: null,
      before: null,
    });
    expect(placementAtContainerEnd(build(), 'missing', 'pinned')).toBeNull();
  });
});

describe('placementForSiblingStep', () => {
  const snap = () => snapshot({ pinned: [tab('a', 'A'), tab('b', 'B'), tab('c', 'C')] });

  it('moves up before the previous sibling', () => {
    expect(placementForSiblingStep(snap(), 'b', -1)).toMatchObject({ before: 'a' });
    expect(placementForSiblingStep(snap(), 'a', -1)).toBeNull();
  });

  it('moves down after the next sibling', () => {
    expect(placementForSiblingStep(snap(), 'a', 1)).toMatchObject({ before: 'c' });
    expect(placementForSiblingStep(snap(), 'b', 1)).toMatchObject({ before: null });
    expect(placementForSiblingStep(snap(), 'c', 1)).toBeNull();
  });
});

describe('normalizeAddress', () => {
  it.each([
    ['https://example.com/a', 'https://example.com/a'],
    ['http://example.com', 'http://example.com/'],
    ['example.com', 'https://example.com/'],
    ['about:blank', 'about:blank'],
    ['  ABOUT:BLANK  ', 'about:blank'],
  ])('accepts %j', (input, url) => {
    expect(normalizeAddress(input)).toStrictEqual({ ok: true, url });
  });

  it.each([
    'javascript:alert(1)',
    'JavaScript:alert(1)',
    '  javascript:alert(1)',
    'java\tscript:alert(1)',
    'file:///etc/passwd',
    'data:text/html,<script>alert(1)</script>',
    'app://ui/sidebar.html',
    'about:srcdoc',
    'ftp://example.com',
    'view-source:https://example.com',
    'mailto:someone@example.com',
    'about:1234',
    'javascript:1',
    'https://',
    'two words',
    '',
    '   ',
  ])('rejects %j', (input) => {
    const result = normalizeAddress(input);
    expect(result.ok).toBe(false);
  });
});

describe('normalizeAddress: host:port, length and characters', () => {
  it.each([
    ['localhost:3000', 'http://localhost:3000/'],
    ['localhost', 'http://localhost/'],
    ['example.com:8080', 'https://example.com:8080/'],
    ['192.168.0.10:8080/admin', 'http://192.168.0.10:8080/admin'],
    ['192.168.1.1', 'http://192.168.1.1/'],
    ['example.com/a?b=1', 'https://example.com/a?b=1'],
  ])('reads %j as a host, not a scheme', (input, url) => {
    expect(normalizeAddress(input)).toStrictEqual({ ok: true, url });
  });

  it('accepts exactly 2048 characters and rejects 2049', () => {
    const host = 'https://example.com/';
    const atLimit = host + 'a'.repeat(2048 - host.length);
    expect(atLimit.length).toBe(2048);
    expect(normalizeAddress(atLimit).ok).toBe(true);
    const overLimit = atLimit + 'a';
    expect(normalizeAddress(overLimit)).toMatchObject({ ok: false });
  });

  it.each(['\u0000', 'https://example.com/\u0007', 'https://example.com/\u007f', 'https://exa\tmple.com'])(
    'rejects control characters in %j',
    (input) => {
      expect(normalizeAddress(input).ok).toBe(false);
    },
  );

  it.each(['   ', '\t\n', '   '])('rejects whitespace-only input %j', (input) => {
    expect(normalizeAddress(input).ok).toBe(false);
  });
});

describe('placements for keyboard moves and the before-drop no-op', () => {
  it('a before-drop onto the next sibling of the dragged node is a no-op and is not sent', () => {
    // pinned: [f, b]. Dragging f before b leaves f where it is.
    const snap = snapshot({ pinned: [folder('f', 'F', []), tab('b', 'B')] });
    expect(placementForRow(snap, 'f', 'b', 'before')).toBeNull();
    expect(placementForRow(snap, 'b', 'f', 'after')).toBeNull();
  });

  it('moves a node out of its folder to just after that folder', () => {
    const snap = snapshot({ pinned: [folder('f', 'F', [tab('c', 'C')]), tab('m', 'M')] });
    expect(placementOutOfFolder(snap, 'c')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: null,
      before: 'm',
    });
    expect(placementOutOfFolder(snap, 'm')).toBeNull();
  });

  it('moves a node into the folder directly above it, and only into a folder', () => {
    const snap = snapshot({ pinned: [folder('f', 'F', []), tab('m', 'M'), tab('x', 'X')] });
    expect(placementIntoPreviousFolder(snap, 'm')).toStrictEqual({
      space: 'space-1',
      container: 'pinned',
      parent: 'f',
      before: null,
    });
    expect(placementIntoPreviousFolder(snap, 'x')).toBeNull();
    expect(placementIntoPreviousFolder(snap, 'f')).toBeNull();
  });
});
