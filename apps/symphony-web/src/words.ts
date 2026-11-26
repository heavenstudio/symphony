// The words the pages say that more than one of them says.

/** What an action's button says while the page may show what is out of date, and it waits. */
export const CHECKING = "Checking…";

/** The months' short names, as a page writes a date (`3 Nov`): January's at 0, as `Date` counts. */
export const MONTHS = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split(" ");

/** The weekdays' short names (`Sun 15 Nov`): Sunday's at 0, as `Date` counts. */
export const WEEKDAYS = "Sun Mon Tue Wed Thu Fri Sat".split(" ");

/** The days from 1970-01-01 to `date`'s day where the owner is. */
function day(date: Date): number {
  return Math.round(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 864e5);
}

/** `n` in two digits at least, as a clock or a date writes it: `09`, `14`, `123`. */
export const twoDigits = (n: number) => String(n).padStart(2, "0");

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

/** What a page says of the task board's own state: nothing unless it is unavailable; that it is
 * slow when it is, else that it is unavailable. */
export function statusNotice(available: boolean | undefined, slow: boolean): string {
  if (available !== false) return "";
  return slow
    ? "The task board is slow to answer. Retrying…"
    : "The task board is unavailable. Reconnecting…";
}

/** `noun`, with an s unless there is one. */
export function plural(count: number, noun: string): string {
  return count === 1 ? noun : `${noun}s`;
}

/** The digits after the point a decimal `digits` (no sign, no point) keeps when rounded up by one. */
function increment(digits: string): string {
  const out = digits.split("");
  for (let at = out.length - 1; at >= 0; at--) {
    if (out[at] !== "9") {
      out[at] = String(Number(out[at]) + 1);
      return out.join("");
    }
    out[at] = "0";
  }
  return `1${out.join("")}`;
}

/**
 * `value` with `places` decimals, as Rust's `{:.N}` writes it (the Dioxus pages' words): the
 * nearest, a tie to the even digit (`2.125` to two places is `2.12`, where JavaScript's `toFixed`
 * says `2.13`), and a negative zero with its sign. The tie is read from the number's own decimal
 * digits, which `toFixed(100)` writes exactly for every value a chart or a percentage holds.
 */
export function fixed(value: number, places: number): string {
  const sign = value < 0 || Object.is(value, -0) ? "-" : "";
  const exact = Math.abs(value).toFixed(100);
  if (!exact.includes(".")) return `${sign}${exact}`;
  const [whole = "0", fraction = ""] = exact.split(".");
  const kept = whole + fraction.slice(0, places);
  const rest = fraction.slice(places);
  const first = rest[0] ?? "0";
  const tie = first === "5" && !/[1-9]/.test(rest.slice(1));
  const up = first > "5" || (first === "5" && !tie) || (tie && Number(kept.at(-1)) % 2 === 1);
  const digits = up ? increment(kept) : kept;
  const point = digits.length - places;
  return places === 0
    ? `${sign}${digits}`
    : `${sign}${digits.slice(0, point)}.${digits.slice(point)}`;
}

/** Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's). */
function daysFromCivil(year: number, month: number, day: number): number {
  const shifted = month <= 2 ? year - 1 : year;
  const era = Math.floor(shifted / 400);
  const yearOfEra = shifted - era * 400;
  const dayOfYear = Math.floor((153 * ((month + 9) % 12) + 2) / 5) + day - 1;
  const dayOfEra =
    yearOfEra * 365 + Math.floor(yearOfEra / 4) - Math.floor(yearOfEra / 100) + dayOfYear;
  return era * 146_097 + dayOfEra - 719_468;
}

/**
 * A wall-clock time without a zone, as the registers write it, in minutes since 1970-01-01 00:00;
 * none when a part is out of range.
 */
export function moment(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
): number | null {
  if (month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || minute > 59) return null;
  return daysFromCivil(year, month, day) * 1440 + hour * 60 + minute;
}

/** `date` on the wall clock where the owner is, as a moment. */
export function localMoment(date: Date): number {
  return (
    moment(
      date.getFullYear(),
      date.getMonth() + 1,
      date.getDate(),
      date.getHours(),
      date.getMinutes(),
    ) ?? 0
  );
}

/** A count as Rust reads one into an unsigned integer: digits, a `+` before them allowed. */
const unsigned = (text: string) => (/^\+?\d+$/.test(text) ? Number(text) : null);

/**
 * `YYYY-MM-DD HH:MM` or `YYYY-MM-DD` as a moment, each part its width and read as Rust reads a
 * number; none for anything else.
 */
export function parseMoment(text: string): number | null {
  const space = text.indexOf(" ");
  const date = space < 0 ? text : text.slice(0, space);
  const time = space < 0 ? null : text.slice(space + 1);
  const parts = date.split("-");
  if (parts.length !== 3) return null;
  const [year = "", month = "", day = ""] = parts;
  if (year.length !== 4 || month.length !== 2 || day.length !== 2) return null;
  let hour: number | null = 0;
  let minute: number | null = 0;
  if (time !== null) {
    const colon = time.indexOf(":");
    if (colon < 0) return null;
    const [hours, minutes] = [time.slice(0, colon), time.slice(colon + 1)];
    if (hours.length !== 2 || minutes.length !== 2) return null;
    [hour, minute] = [unsigned(hours), unsigned(minutes)];
  }
  // The year is signed in Rust, but a `-` in it splits the date: a `+` is all it can take.
  const [years, months, days] = [unsigned(year), unsigned(month), unsigned(day)];
  if (years === null || months === null || days === null || hour === null || minute === null) {
    return null;
  }
  return moment(years, months, days, hour, minute);
}
