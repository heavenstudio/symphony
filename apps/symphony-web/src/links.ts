// Which addresses a page may make a link of. The addresses a page links come from text it did not
// write (a plugin's gate, an agent's session), and the projects of the machine's one server share
// its origin: a `javascript:` link run there could act on any of them. So a page links only a path
// of the app or a web address, and draws anything else as text; the server checks the same where it
// can, so no page is handed one.
//
// `appPath`, `webAddress` and `linkable` carry crates/symphony-api's own (`links.rs`'s `app_path`
// and `linkable`, `agents.rs`'s `web_address`), held to the Rust by its tests, ported to
// links.test.ts.

/** Rust's `char::is_control`: a character of the general category Cc. */
const CONTROL = /\p{Cc}/u;

/** Rust's `char::is_whitespace` (the White_Space property) or `char::is_control`. */
const SPACE_OR_CONTROL = /[\p{White_Space}\p{Cc}]/u;

/** `text` with its ASCII capitals made small, every other character left as it is. */
function asciiLower(text: string): string {
  return text.replace(/[A-Z]/g, (capital) => capital.toLowerCase());
}

/**
 * Whether `path` is a path of this app (`/task/SLUG#message-ID`, `/docs/X.md#card`): one `/` first,
 * with no backslash or control character anywhere, since a browser reads `//host`, `/\host` and a
 * `/` a tab or newline split from the next as another host.
 */
export function appPath(path: string): boolean {
  return (
    path.startsWith("/") && !path.startsWith("//") && !path.includes("\\") && !CONTROL.test(path)
  );
}

/**
 * Whether a page may link to `url` as a session's web address: an `http://` or `https://` URL (in
 * any case) naming a host, with no space or control character in it.
 */
export function webAddress(url: string): boolean {
  const rest = ["https://", "http://"]
    .filter((scheme) => asciiLower(url.slice(0, scheme.length)) === scheme)
    .map((scheme) => url.slice(scheme.length))[0];
  // The host is what comes before the first `/`, `?` or `#`: none when one of them comes first.
  return rest !== undefined && !SPACE_OR_CONTROL.test(url) && /^[^/?#]/.test(rest);
}

/** Whether a page may link to `link`: a path of the app (`appPath`) or a web address. */
export function linkable(link: string): boolean {
  return appPath(link) || webAddress(link);
}

/**
 * The slug of the task whose page `path` names, where the server writes one into its words (the
 * read-only notice, a schema update's task): an app path holding `/task/` (`/task/SLUG`, or
 * `/<project>/task/SLUG` on the machine's one server), its slug what follows that, up to a `?` or
 * `#`; none for any other text. The client's own: no Rust carries it.
 */
export function taskSlug(path: string): string | null {
  const at = path.indexOf("/task/");
  if (!appPath(path) || at < 0) return null;
  const slug = path.slice(at + "/task/".length).split(/[?#]/)[0] ?? "";
  return slug === "" ? null : slug;
}
