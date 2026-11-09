/** Follow the OS appearance and motion settings; nothing is stored. */
export function applyTheme(): void {
  const root = document.documentElement;
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const sync = () => {
    root.dataset["theme"] = dark.matches ? "dark" : "light";
    root.dataset["motion"] = motion.matches ? "reduced" : "full";
  };
  sync();
  dark.addEventListener("change", sync);
  motion.addEventListener("change", sync);
}
