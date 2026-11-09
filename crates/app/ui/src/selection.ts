/** View state shared by the Run panel's plots and arithmetic: the replay
 * stage and the selected observation. Everything else lives in Rust. */
export interface Selection {
  stage: number;
  selected: number | null;
  feature: number;
}

type Listener = (s: Selection) => void;

export class SelectionStore {
  private state: Selection = { stage: 0, selected: null, feature: 0 };
  private listeners = new Set<Listener>();

  get(): Selection {
    return { ...this.state };
  }

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** Apply a partial change; listeners are called once, and only if something changed. */
  set(change: Partial<Selection>): void {
    const next = { ...this.state, ...change };
    if (next.stage === this.state.stage && next.selected === this.state.selected && next.feature === this.state.feature) return;
    this.state = next;
    const snapshot = this.get();
    this.listeners.forEach((fn) => fn(snapshot));
  }
}
