import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Sidebar } from '../state/sidebar';
import { NEW_TAB_BUTTON_ID } from './ids';
import { PlusIcon } from './icons';

interface Props {
  sidebar: Sidebar;
}

/** New tab, and new folder with an inline name. The folder is created at the root of `today`. */
export function Toolbar({ sidebar }: Props) {
  const [folderName, setFolderName] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const open = folderName !== null;

  // Preact does not move focus for `autoFocus` on an element inserted after load.
  useLayoutEffect(() => {
    if (open) field.current?.focus();
  }, [open]);

  const submitFolder = () => {
    if (folderName === null) return;
    const sent = sidebar.createFolder('today', folderName);
    // Close the field only when the core accepted the folder. The rename then starts on it.
    void sent.then((result) => {
      if (result?.status === 'ok') setFolderName(null);
    });
  };

  return (
    <div class="toolbar">
      <button id={NEW_TAB_BUTTON_ID} type="button" class="button" onClick={() => void sidebar.newTab()}>
        <PlusIcon />
        New tab
      </button>
      <button
        type="button"
        class="button"
        aria-expanded={folderName !== null}
        onClick={() => setFolderName(folderName === null ? '' : null)}
      >
        New folder
      </button>
      {folderName !== null ? (
        <input
          ref={field}
          type="text"
          class="text-field folder-name"
          aria-label="Folder name"
          placeholder="Folder name"
          value={folderName}
          onInput={(event) => setFolderName(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              submitFolder();
            } else if (event.key === 'Escape') {
              setFolderName(null);
            }
          }}
        />
      ) : null}
    </div>
  );
}
