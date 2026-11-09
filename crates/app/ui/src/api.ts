import { invoke } from "@tauri-apps/api/core";

export type Level = "Beginner" | "Intermediate" | "Advanced";

export interface LessonCard {
  id: string;
  title: string;
  kind: "run" | "by_hand";
  level: Level;
  estimated_minutes: number;
  prerequisites: string[];
  prerequisite_titles: string[];
  concepts: string[];
  status: string | null;
}

export interface LessonError {
  file: string;
  field: string;
  message: string;
}

export interface ReadPanel {
  title: string;
  html: string;
  derivation_html: string[];
  instructor_html: string | null;
  missing: string[];
  status: string | null;
}

export interface Learner {
  field: string;
  min: number;
  max: number;
  step: number;
}

export interface Table {
  columns: string[];
  rows: unknown[][];
}

export interface Plot {
  id: string;
  title: string;
  description: string;
  spec: unknown;
  table: Table;
}

export interface Observation {
  row: number;
  x: [string, number][];
  y: number;
  before: number;
  h: number | null;
  rate: number | null;
  after: number;
  residual_before: number;
  residual_after: number;
  rules: string[];
  in_training: boolean;
}

export type Status = "Running" | "Done" | "Canceled" | { Failed: string };

export interface RunPanel {
  run_id: string;
  lesson_id: string;
  stages: number;
  stage: number;
  best_stage: number | null;
  features: string[];
  plots: Plot[];
  observation: Observation | null;
  controls: Learner[];
  config: Record<string, unknown>;
  context: unknown;
  kind: "run" | "by_hand";
}

export type JobStatus = "running" | "interrupted" | "completed" | "failed" | "canceled";

export interface Manifest {
  id: string;
  label: string;
  lesson_id: string;
  created_ms: number;
  status: JobStatus;
  units_total: number | null;
  last_checkpoint_stage: number | null;
  units_done: number;
  error: string | null;
}

export interface Sample {
  at_ms: number;
  cpu_percent: number;
  rss_bytes: number;
  processes: number;
  gpu_memory_bytes: number | null;
}

export interface DurationJson {
  secs: number;
  nanos: number;
}

export type JobEvent =
  | { event: "started"; pid: number; started_at_ms: number; limits: string }
  | {
      event: "progress";
      units_done: number;
      units_total: number | null;
      unit: string;
      stage: string | null;
      eta: { low: DurationJson; high: DurationJson; units_observed: number; units_remaining: number } | null;
      latest: Sample | null;
    }
  | { event: "output"; [k: string]: unknown }
  | ({ event: "sample" } & Sample)
  | { event: "checkpoint"; path: string; sha256: string }
  | { event: "completed"; result: unknown; wall: DurationJson }
  | { event: "failed"; error: string; limit: string | null; wall: DurationJson };

export type PredictionKind = "direction" | { number: { tolerance: number } } | { choice: { options: string[] } };

export interface Reveal {
  actual: string;
  correct: boolean;
  explanation_html: string;
  saved: boolean;
}

export interface Answered {
  chosen: number[];
  correct: boolean;
  explanation_html: string;
  from_earlier_setting: boolean;
  saved: boolean;
}

export interface PredictionView {
  id: string;
  prompt_html: string;
  kind: PredictionKind;
  committed: string | null;
  revealed: Reveal | null;
}

export interface QuestionView {
  id: string;
  concept: string;
  stem_html: string;
  options_html: string[];
  order: number[];
  multiple: boolean;
  answered: Answered | null;
  locked_until_prediction: string | null;
}

export interface CheckPanel {
  predictions: PredictionView[];
  questions: QuestionView[];
  record_notice: string | null;
  history: { attempts: number; predictions: number; answers: number; correct_answers: number; last_attempt_ms: number | null };
  mastered: string[];
}

export interface PacketFiles {
  dir: string;
  files: string[];
  figure_ids: string[];
}

export const api = {
  exportPacket: (lessonId: string, runId: string, studentOnly: boolean) =>
    invoke<PacketFiles>("export_packet", { lessonId, runId, studentOnly }),
  saveFigure: (packetDir: string, id: string, svg: string) => invoke<string>("save_figure", { packetDir, id, svg }),
  checkPanel: (lessonId: string, runId: string) => invoke<CheckPanel>("check_panel", { lessonId, runId }),
  commitPrediction: (lessonId: string, runId: string, promptId: string, predicted: string) =>
    invoke<Reveal>("commit_prediction", { lessonId, runId, promptId, predicted }),
  answerQuestion: (lessonId: string, runId: string, questionId: string, chosen: number[]) =>
    invoke<Answered>("answer_question", { lessonId, runId, questionId, chosen }),
  exportRecord: () => invoke<string>("export_record"),
  startJob: (lessonId: string, csvPath: string, overrides: Record<string, number>) =>
    invoke<string>("start_job", { lessonId, csvPath, overrides }),
  listJobs: () => invoke<Manifest[]>("list_jobs"),
  resumeJob: (jobId: string) => invoke<string>("resume_job", { jobId }),
  cancelJob: (jobId: string) => invoke<void>("cancel_job", { jobId }),
  openJobResult: (jobId: string) => invoke<string>("open_job_result", { jobId }),
  startExperiment: (lessonId: string, overrides: Record<string, number>) =>
    invoke<string>("start_experiment", { lessonId, overrides }),
  experimentStatus: (runId: string) => invoke<Status>("experiment_status", { runId }),
  cancelExperiment: (runId: string) => invoke<void>("cancel_experiment", { runId }),
  runPanel: (runId: string, stage: number, feature: number, selected: number | null) =>
    invoke<RunPanel>("run_panel", { runId, stage, feature, selected }),
  readPanelForRun: (runId: string, stage: number, selected: number | null) =>
    invoke<ReadPanel>("read_panel_for_run", { runId, stage, selected }),
  lessonControls: (id: string) => invoke<Learner[]>("lesson_controls", { id }),
  listLessons: () => invoke<LessonCard[]>("list_lessons"),
  lessonErrors: () => invoke<LessonError[]>("lesson_errors"),
  readPanel: (id: string) => invoke<ReadPanel>("read_panel", { id }),
  setInstructor: (on: boolean) => invoke<boolean>("set_instructor", { on }),
  instructor: () => invoke<boolean>("instructor"),
};
