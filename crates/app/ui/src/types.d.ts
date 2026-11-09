declare module "katex" {
  export interface KatexOptions {
    throwOnError?: boolean;
    displayMode?: boolean;
    output?: "html" | "mathml" | "htmlAndMathml";
  }
  const katex: {
    render(tex: string, element: HTMLElement, options?: KatexOptions): void;
    renderToString(tex: string, options?: KatexOptions): string;
  };
  export default katex;
}
declare module "katex/dist/katex.min.css";
