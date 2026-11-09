/** The current run per lesson, for this window's lifetime. Rust holds the
 * runs themselves; this only remembers which one each lesson shows. */
export interface CurrentRun {
  runId: string;
}

const current = new Map<string, CurrentRun>();

export const runs = {
  get: (lessonId: string): CurrentRun | undefined => current.get(lessonId),
  set: (lessonId: string, run: CurrentRun): void => void current.set(lessonId, run),
  clear: (lessonId: string): void => void current.delete(lessonId),
};
