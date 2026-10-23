import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Sidebar } from '../state/sidebar';
import type { Snapshot } from '../wire';
import { PlusIcon } from './icons';

interface Props {
  sidebar: Sidebar;
  snapshot: Snapshot;
}

/**
 * Lists the Spaces and marks the active one. The wire has no command to change the active
 * Space yet, so the list is informational. Creating a Space is inline: no `prompt()`.
 */
export function SpaceSwitcher({ sidebar, snapshot }: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const field = useRef<HTMLInputElement>(null);

  // Preact does not move focus for `autoFocus` on an element inserted after load.
  useLayoutEffect(() => {
    if (adding) field.current?.focus();
  }, [adding]);

  const close = () => {
    setAdding(false);
    setName('');
  };

  const submit = () => {
    const trimmed = name.trim();
    if (trimmed === '') return;
    void sidebar.createSpace(trimmed);
    close();
  };

  return (
    <header class="spaces">
      <ul class="space-list" aria-label="Spaces">
        {snapshot.spaces.map((space) => {
          const active = space.id === snapshot.active_space;
          return (
            <li key={space.id} class={active ? 'space active' : 'space'} aria-current={active ? 'true' : undefined}>
              {space.name}
            </li>
          );
        })}
      </ul>
      <button
        type="button"
        class="icon-button"
        aria-label="New space"
        aria-expanded={adding}
        onClick={() => (adding ? close() : setAdding(true))}
      >
        <PlusIcon />
      </button>
      {adding ? (
        <input
          ref={field}
          type="text"
          class="text-field space-name"
          aria-label="Space name"
          placeholder="Space name"
          value={name}
          onInput={(event) => setName(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              submit();
            } else if (event.key === 'Escape') {
              close();
            }
          }}
        />
      ) : null}
    </header>
  );
}
