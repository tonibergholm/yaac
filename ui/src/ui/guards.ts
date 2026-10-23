import { isInternalDrag } from './drag';

/**
 * Stops the page from navigating because of something dropped on it from outside.
 *
 * Chromium navigates to a dropped file or URL unless the page cancels the drag. Every drag that is
 * not internal therefore gets these handlers, in the capture phase at the window, so no element can
 * let it through:
 *
 * - `dragenter` and `dragover` are cancelled, and `dropEffect` is set to `none`, so the "not
 *   allowed" cursor shows and no drop is accepted.
 * - `drop` is cancelled as a second layer.
 *
 * `dragleave` is not cancelable, so it has no handler here.
 *
 * Internal tree drags are left alone here. Rows and containers accept them only on valid targets,
 * which keeps the invalid-drop feedback. Every internal drop is validated again at commit.
 *
 * Not handled: `beforeunload`. A handler there can trap the user in a confirm prompt, so it is left
 * out. Middle clicks are cancelled through `auxclick`. Paste is left alone, since it does not
 * navigate and it is how text reaches the address field.
 */
export function guardForeignDrops(target: Window = window): void {
  const refuse = (event: DragEvent) => {
    if (isInternalDrag(event.dataTransfer)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'none';
  };
  target.addEventListener('dragenter', refuse, { capture: true });
  target.addEventListener('dragover', refuse, { capture: true });
  target.addEventListener('drop', (event) => event.preventDefault(), { capture: true });
  target.addEventListener('auxclick', (event) => event.preventDefault(), { capture: true });
}
