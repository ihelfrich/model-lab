import { expect, test } from "vitest";
import { SelectionStore } from "./selection";

test("subscribers are called once per change and not for no-ops", () => {
  const store = new SelectionStore();
  let calls = 0;
  store.subscribe(() => (calls += 1));
  store.set({ stage: 3 });
  store.set({ stage: 3 });
  store.set({ selected: 7 });
  store.set({ selected: 7, stage: 3 });
  expect(calls).toBe(2);
  expect(store.get()).toEqual({ stage: 3, selected: 7, feature: 0 });
});

test("unsubscribe stops notifications", () => {
  const store = new SelectionStore();
  let calls = 0;
  const off = store.subscribe(() => (calls += 1));
  store.set({ stage: 1 });
  off();
  store.set({ stage: 2 });
  expect(calls).toBe(1);
});
