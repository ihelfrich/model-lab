/** Small element builder: attributes with a null value are omitted. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | null> = {},
  ...children: (string | Node)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v !== null) node.setAttribute(k, v);
  }
  node.append(...children);
  return node;
}

export function announce(text: string): void {
  const live = document.getElementById("live");
  if (live) live.textContent = text;
}
