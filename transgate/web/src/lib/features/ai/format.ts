// Display formatting the source's section components did inline. Copy text is
// formatted in core (section_copy.rs); these only draw the on-screen values.

/** `m:ss` from seconds, as the source's `formatTimestamp`. */
export function clock(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

/** Seconds under a minute, otherwise whole minutes (`formatDuration`). */
export function duration(seconds: number): string {
  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)} min`;
}

/** A local date for epoch microseconds, as `toLocaleDateString()` showed. */
export function localDate(micros: number, locale: string): string {
  return new Date(micros / 1000).toLocaleDateString(locale);
}
