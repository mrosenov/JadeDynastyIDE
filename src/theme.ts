import type { Theme } from "./elements/types";

/** Applies a saved theme. With no attribute, CSS follows the operating system. */
export function applyTheme(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}
