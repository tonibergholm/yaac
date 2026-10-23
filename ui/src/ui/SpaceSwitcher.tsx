import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Sidebar } from '../state/sidebar';
import type { Snapshot } from '../wire';
import { PlusIcon } from './icons';

interface Props {
  sidebar: Sidebar;
  snapshot: Snapshot;
}

/**
 * Lists the Spaces. Clicking one switches the window to it (`select_space`). The active Space is
 * marked with aria-current. New Space is an inline name field, and the core switches to the new
 * Space once it is created.
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
            <li key={space.id}>
              <button
                type="button"
                class="space"
                aria-current={active ? 'true' : undefined}
                onClick={() => void sidebar.selectSpace(space.id)}
              >
                {space.name}
              </button>
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
