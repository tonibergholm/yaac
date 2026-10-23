/**
 * A link or file dragged in from outside the page would navigate the sidebar if dropped on it.
 * Cancel `dragover` and `drop` at the window, in the capture phase, so no element can let such a
 * drop through. Internal drops are unaffected: rows and containers decide validity themselves.
 */
export function guardForeignDrops(target: Window = window): void {
  for (const type of ['dragover', 'drop'] as const) {
    target.addEventListener(type, (event) => event.preventDefault(), { capture: true });
  }
}
