import { invoke } from "@tauri-apps/api/core";

export type Level = "Beginner" | "Intermediate" | "Advanced";

export interface LessonCard {
  id: string;
  title: string;
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

export const api = {
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
