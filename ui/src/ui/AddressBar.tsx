import { useLayoutEffect, useState } from 'preact/hooks';
import { normalizeAddress } from '../model/address';
import type { Sidebar } from '../state/sidebar';
import type { NodeView } from '../wire';

interface Props {
  sidebar: Sidebar;
  selected: NodeView | null;
}

/**
 * Address of the selected tab. Accepts http, https and about:blank only. Enter navigates; a
 * rejected address shows an inline error and sends nothing.
 */
export function AddressBar({ sidebar, selected }: Props) {
  const url = selected?.url ?? '';
  const [draft, setDraft] = useState(url);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Follow the core's URL unless the user is typing in the field. A layout effect runs during
  // commit. Passive effects run later, and they could reset what the user types right after a mount.
  useLayoutEffect(() => {
    if (!editing) {
      setDraft(url);
      setError(null);
    }
  }, [url, editing]);

  const submit = () => {
    if (!selected) return;
    const result = normalizeAddress(draft);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setError(null);
    void sidebar.navigate(selected.id, result.url);
  };

  return (
    <div class="address">
      <input
        type="text"
        class="text-field"
        aria-label="Address"
        aria-invalid={error !== null}
        aria-describedby={error !== null ? 'address-error' : undefined}
        placeholder={selected ? 'Address' : 'No tab selected'}
        disabled={selected === null}
        value={draft}
        spellcheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        onInput={(event) => {
          setDraft(event.currentTarget.value);
          setError(null);
        }}
        onFocus={() => setEditing(true)}
        onBlur={() => setEditing(false)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            submit();
          } else if (event.key === 'Escape') {
            setDraft(url);
            setError(null);
            event.currentTarget.blur();
          }
        }}
      />
      {error !== null ? (
        <p id="address-error" class="error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
