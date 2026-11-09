import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type JobEvent, type LessonCard, type Manifest } from "../api";
import { announce, el } from "../dom";
import { runs } from "../runs";

const fmtBytes = (b: number) => (b >= 1 << 30 ? `${(b / (1 << 30)).toFixed(2)} GiB` : `${(b / (1 << 20)).toFixed(0)} MiB`);
const fmtSecs = (s: number) => (s >= 3600 ? `${(s / 3600).toFixed(1)} h` : s >= 60 ? `${Math.round(s / 60)} min` : `${Math.round(s)} s`);

interface Live {
  units_done: number;
  units_total: number | null;
  eta: string;
  cpu: string;
  rss: string;
  processes: string;
  checkpoint: string;
}

function liveLine(ev: JobEvent, prev: Live): Live {
  const next = { ...prev };
  if (ev.event === "progress") {
    next.units_done = ev.units_done;
    next.units_total = ev.units_total;
    next.eta = ev.eta
      ? `between ${fmtSecs(ev.eta.low.secs)} and ${fmtSecs(ev.eta.high.secs)} (from ${ev.eta.units_observed} observed trees)`
      : `estimating after 3 trees (${ev.units_done} so far)`;
    if (ev.latest) {
      next.cpu = `${ev.latest.cpu_percent.toFixed(0)} %`;
      next.rss = fmtBytes(ev.latest.rss_bytes);
      next.processes = String(ev.latest.processes);
    }
  } else if (ev.event === "sample") {
    next.cpu = `${ev.cpu_percent.toFixed(0)} %`;
    next.rss = fmtBytes(ev.rss_bytes);
    next.processes = String(ev.processes);
  } else if (ev.event === "checkpoint") {
    next.checkpoint = `tree ${prev.units_done}`;
  }
  return next;
}

/** The Jobs route: start a supervised job on a CSV, watch it, resume it, open its result. */
export async function renderJobs(host: HTMLElement): Promise<() => void> {
  host.replaceChildren(el("h1", {}, "Jobs"));
  host.append(
    el(
      "p",
      { class: "muted" },
      "A supervised job fits a lesson's model on your own CSV (up to 50,000 rows) in a separate process with limits, checkpoints and a resume after a quit.",
    ),
  );
  const [lessons, manifests] = await Promise.all([api.listLessons(), api.listJobs()]);
  const dataLessons = lessons.filter((l) => l.kind === "run");
  host.append(startForm(dataLessons, () => void refresh()));
  const list = el("div", { class: "jobs" });
  host.append(list);
  const live = new Map<string, Live>();
  const unlisteners: UnlistenFn[] = [];
  let lastAnnounce = 0;

  async function refresh(): Promise<void> {
    const all = await api.listJobs();
    list.replaceChildren(...all.map((m) => jobCard(m, live.get(m.id))));
    for (const m of all) {
      if (m.status === "running" && !live.has(m.id)) {
        live.set(m.id, { units_done: m.units_done, units_total: m.units_total, eta: "estimating after 3 trees", cpu: "–", rss: "–", processes: "–", checkpoint: m.last_checkpoint_stage === null ? "none yet" : `tree ${m.last_checkpoint_stage}` });
        unlisteners.push(
          await listen<JobEvent>(`job://${m.id}`, (e) => {
            const prev = live.get(m.id);
            if (!prev) return;
            const next = liveLine(e.payload, prev);
            live.set(m.id, next);
            const card = list.querySelector<HTMLElement>(`[data-job="${m.id}"]`);
            if (card) card.replaceWith(jobCard(m, next));
            const now = Date.now();
            if (now - lastAnnounce > 5000) {
              lastAnnounce = now;
              announce(`Job ${m.label}: ${next.units_done} of ${next.units_total ?? "?"} trees, ${next.eta}`);
            }
            if (e.payload.event === "completed" || e.payload.event === "failed") void refresh();
          }),
        );
      }
    }
  }

  function jobCard(m: Manifest, l: Live | undefined): HTMLElement {
    const card = el("section", { class: "card job", "data-job": m.id, "aria-label": m.label });
    card.append(el("h2", {}, m.label), el("p", { class: "card-meta" }, `${m.status}${m.error ? `: ${m.error}` : ""}`));
    const done = l?.units_done ?? m.units_done;
    card.append(el("p", {}, `Trees: ${done} of ${m.units_total ?? "?"}. Last checkpoint: ${l?.checkpoint ?? (m.last_checkpoint_stage === null ? "none yet" : `tree ${m.last_checkpoint_stage}`)}.`));
    if (m.status === "running") {
      card.append(el("p", {}, `ETA ${l?.eta ?? "estimating after 3 trees"}.`), el("p", { class: "mono" }, `CPU ${l?.cpu ?? "–"} · memory ${l?.rss ?? "–"} · processes ${l?.processes ?? "–"}`));
      const cancel = el("button", { type: "button", class: "button" }, "Cancel");
      cancel.addEventListener("click", () => void api.cancelJob(m.id).then(refresh));
      card.append(cancel);
    }
    if (m.status === "interrupted" || m.status === "canceled" || m.status === "failed") {
      const resume = el("button", { type: "button", class: "button primary" }, "Resume from the last checkpoint");
      resume.addEventListener("click", () => void api.resumeJob(m.id).then(refresh));
      card.append(resume);
    }
    if (m.status === "completed") {
      const openResult = el("button", { type: "button", class: "button primary" }, "Open result");
      openResult.addEventListener("click", async () => {
        const runId = await api.openJobResult(m.id);
        runs.set(m.lesson_id, { runId });
        location.hash = `#/lesson/${encodeURIComponent(m.lesson_id)}/run`;
      });
      card.append(openResult);
    }
    return card;
  }

  await refresh();
  return () => unlisteners.forEach((u) => u());
}

function startForm(lessons: LessonCard[], onStarted: () => void): HTMLElement {
  const form = el("form", { class: "controls", "aria-label": "Start a supervised job" });
  const select = el("select", { "aria-label": "Lesson whose model to fit" }) as HTMLSelectElement;
  for (const l of lessons) select.append(el("option", { value: l.id }, l.title));
  const file = el("button", { type: "button", class: "button" }, "Choose a CSV…");
  const chosen = el("span", { class: "muted" }, "no file chosen");
  let path: string | null = null;
  file.addEventListener("click", async () => {
    const picked = await open({ multiple: false, filters: [{ name: "CSV", extensions: ["csv"] }] });
    path = typeof picked === "string" ? picked : null;
    chosen.textContent = path ?? "no file chosen";
  });
  const start = el("button", { type: "submit", class: "button primary" }, "Start job");
  const note = el("p", { class: "notice", hidden: "" });
  form.append(el("label", {}, "Lesson: ", select), file, chosen, start, note);
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (!path) {
      note.hidden = false;
      note.textContent = "Choose a CSV first.";
      return;
    }
    try {
      await api.startJob(select.value, path, {});
      note.hidden = true;
      onStarted();
    } catch (err) {
      note.hidden = false;
      note.textContent = String(err);
    }
  });
  return form;
}
