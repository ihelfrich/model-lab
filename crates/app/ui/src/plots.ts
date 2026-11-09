import vegaEmbed, { type Result } from "vega-embed";
import { vegaOptions } from "./vega";
import { el } from "./dom";
import type { Plot } from "./api";

export interface PlotHandle {
  element: HTMLElement;
  result: Result | null;
}

const number = new Intl.NumberFormat(undefined, { maximumSignificantDigits: 6 });

function cell(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "number") return Number.isInteger(v) ? String(v) : number.format(v);
  return String(v);
}

/** The accessible table behind a plot: same rows the plot draws. */
export function tableFor(plot: Plot): HTMLTableElement {
  const table = el("table", { class: "data-table" });
  table.append(el("caption", {}, `${plot.title}: ${plot.table.rows.length} rows`));
  const head = el("thead");
  head.append(el("tr", {}, ...plot.table.columns.map((c) => el("th", { scope: "col" }, c))));
  const body = el("tbody");
  for (const row of plot.table.rows) body.append(el("tr", {}, ...row.map((v) => el("td", {}, cell(v)))));
  table.append(head, body);
  return table;
}

function csvOf(plot: Plot): string {
  const quote = (s: string) => (/[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
  const lines = [plot.table.columns.map(quote).join(",")];
  for (const row of plot.table.rows) lines.push(row.map((v) => quote(cell(v))).join(","));
  return lines.join("\n");
}

/**
 * Render one plot with vega-embed, a description, a data-table toggle and a
 * CSV download. `onSelectRow` receives the clicked observation's row index.
 */
export async function renderPlot(
  plot: Plot,
  host: HTMLElement,
  onSelectRow?: (row: number) => void,
): Promise<PlotHandle> {
  const figure = el("figure", { class: "plot" });
  const chart = el("div", { class: "plot-chart", role: "img", "aria-label": `${plot.title}. ${plot.description}` });
  const caption = el("figcaption", {}, plot.description);
  const toggle = el("button", { type: "button", class: "button", "aria-expanded": "false" }, "Show data table");
  const download = el("a", { class: "button", download: `${plot.id}.csv` }, "Download CSV");
  download.setAttribute("href", `data:text/csv;charset=utf-8,${encodeURIComponent(csvOf(plot))}`);
  const tableHost = el("div", { class: "table-host", hidden: "" });
  toggle.addEventListener("click", () => {
    const open = tableHost.hidden;
    tableHost.hidden = !open;
    toggle.setAttribute("aria-expanded", String(open));
    toggle.textContent = open ? "Hide data table" : "Show data table";
    if (open && !tableHost.firstChild) tableHost.append(tableFor(plot));
  });
  figure.append(chart, caption, el("div", { class: "plot-actions" }, toggle, download), tableHost);
  host.append(figure);
  let result: Result | null = null;
  try {
    result = await vegaEmbed(chart, plot.spec as Record<string, unknown>, vegaOptions);
    if (onSelectRow) {
      try {
        result.view.addSignalListener("select_row", (_name, value) => {
          const v = value as { row?: number[] } | undefined;
          const row = v?.row?.[0];
          if (typeof row === "number") onSelectRow(row);
        });
      } catch {
        // the spec has no selection parameter; nothing to listen to
      }
    }
  } catch (e) {
    chart.append(el("p", { class: "error" }, `Plot could not render: ${String(e)}`));
    tableHost.hidden = false;
    if (!tableHost.firstChild) tableHost.append(tableFor(plot));
  }
  return { element: figure, result };
}
