type Listener = () => void;

/** A minimal observable store. `get` returns a stable snapshot until the next `update`. */
export class Store<S> {
  private value: S;
  private readonly listeners = new Set<Listener>();

  constructor(initial: S) {
    this.value = initial;
  }

  get = (): S => this.value;

  update(patch: Partial<S>): void {
    this.value = { ...this.value, ...patch };
    for (const listener of [...this.listeners]) listener();
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
}
