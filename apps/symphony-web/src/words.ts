// The words the pages say that more than one of them says.

/** What an action's button says while the page may show what is out of date, and it waits. */
export const CHECKING = "Checking…";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** The days from 1970-01-01 to `date`'s day where the owner is. */
function day(date: Date): number {
  return Math.round(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 864e5);
}

const twoDigits = (n: number) => String(n).padStart(2, "0");

/**
 * When a copy kept on this device was saved, as a page says it at `now`, both on the wall clock
 * where the owner is: `14:32` the same day, `yesterday 14:32` the day before, else `3 Nov 14:32`,
 * with the year when it is not `now`'s.
 */
export function savedAt(saved: Date, now: Date): string {
  const clock = `${twoDigits(saved.getHours())}:${twoDigits(saved.getMinutes())}`;
  const days = day(now) - day(saved);
  if (days === 0) return clock;
  if (days === 1) return `yesterday ${clock}`;
  const date = `${saved.getDate()} ${MONTHS[saved.getMonth()]}`;
  return saved.getFullYear() === now.getFullYear()
    ? `${date} ${clock}`
    : `${date} ${saved.getFullYear()} ${clock}`;
}

/**
 * What a page drawn from what this device kept says until the server has answered every read it
 * drew: that it is checking, or, while a read fails and its feed is not open, that the server is
 * not reachable. `saved` is when the oldest of them was kept (`savedAt`).
 */
export function deviceNotice(saved: string, unreachable: boolean): string {
  return unreachable
    ? `Not reachable: shown from this device, saved ${saved}`
    : `Shown from this device, saved ${saved}. Checking for changes…`;
}
