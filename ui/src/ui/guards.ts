/**
 * Cancels every `drop` at the window, in the capture phase, so a link or file dropped from outside
 * cannot navigate the page. `dragover` is not touched: rows and containers accept it only on valid
 * targets, which keeps the "not allowed" feedback. Each drop is validated again at commit.
 */
export function guardForeignDrops(target: Window = window): void {
  target.addEventListener('drop', (event) => event.preventDefault(), { capture: true });
}
