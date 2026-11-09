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

export const api = {
  listLessons: () => invoke<LessonCard[]>("list_lessons"),
  lessonErrors: () => invoke<LessonError[]>("lesson_errors"),
  readPanel: (id: string) => invoke<ReadPanel>("read_panel", { id }),
  setInstructor: (on: boolean) => invoke<boolean>("set_instructor", { on }),
  instructor: () => invoke<boolean>("instructor"),
};
