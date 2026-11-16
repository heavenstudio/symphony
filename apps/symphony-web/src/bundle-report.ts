// The bundle report (report.sh OUT): what each kind of a build makes a browser fetch, read from
// Vite's manifest (`.vite/manifest.json`) as package.sh reads it. A kind's first visit is the
// shared stylesheet, the CSS its entry and the chunks it imports carry, its script and those
// chunks; what it imports with `import()`, at any depth, comes later and is listed apart. The
// shell and the page are not Vite's and are not counted: the budget test weighs a whole first
// visit as a browser makes it (apps/symphony-web-tests/tests/browser).

/** One chunk of Vite's manifest. */
export interface Chunk {
  file: string;
  name?: string;
  isEntry?: boolean;
  imports?: string[];
  dynamicImports?: string[];
  css?: string[];
}

/** Vite's manifest: each chunk by its source's path, or by its file's name for a shared chunk. */
export type Manifest = Record<string, Chunk>;

/** A file's bytes as built and as the server sends them. */
export interface Size {
  raw: number;
  gzip: number;
}

/** The manifest's entry for the stylesheet every kind loads (package.sh names the same). */
export const SHARED = "src/styles/shared.css";

/** What one kind fetches: on its first visit, and once a deferred part is asked for. */
export interface Kind {
  kind: string;
  first: string[];
  later: string[];
}

function chunk(manifest: Manifest, key: string): Chunk {
  const found = manifest[key];
  if (!found) throw new Error(`the manifest names ${key} and holds no such chunk`);
  return found;
}

/** `key` and every chunk it imports statically at any depth, each once, imports first. */
function withImports(manifest: Manifest, key: string, seen: Set<string>): string[] {
  if (seen.has(key)) return [];
  seen.add(key);
  const imports = (chunk(manifest, key).imports ?? []).flatMap((item) =>
    withImports(manifest, item, seen),
  );
  return [...imports, key];
}

/** The files `keys` are: their CSS first, as a page links it, then their scripts. */
function files(manifest: Manifest, keys: string[]): string[] {
  const css = keys.flatMap((key) => chunk(manifest, key).css ?? []);
  const scripts = keys.map((key) => chunk(manifest, key).file);
  return [...new Set([...css, ...scripts])];
}

/** The kinds of `manifest`, by name: every entry but the shared stylesheet's. */
export function kinds(manifest: Manifest): Kind[] {
  const shared = manifest[SHARED];
  if (!shared) throw new Error(`the manifest has no shared stylesheet (${SHARED})`);
  const entries = Object.keys(manifest)
    .filter((key) => manifest[key]?.isEntry && key !== SHARED)
    .sort((a, b) => kindName(manifest, a).localeCompare(kindName(manifest, b)));
  if (entries.length === 0) throw new Error("the manifest has no kind");
  return entries.map((entry) => {
    const seen = new Set<string>();
    const first = withImports(manifest, entry, seen);
    // Each `import()` of a chunk already reached, and of those it brings, until none is new.
    const later: string[] = [];
    let from = first;
    while (from.length > 0) {
      const next = from
        .flatMap((key) => chunk(manifest, key).dynamicImports ?? [])
        .flatMap((key) => withImports(manifest, key, seen));
      later.push(...next);
      from = next;
    }
    const firstFiles = [...new Set([shared.file, ...files(manifest, first)])];
    return {
      kind: kindName(manifest, entry),
      first: firstFiles,
      later: files(manifest, later).filter((file) => !firstFiles.includes(file)),
    };
  });
}

function kindName(manifest: Manifest, entry: string): string {
  return chunk(manifest, entry).name ?? entry;
}

function total(names: string[], size: (file: string) => Size): string {
  const sizes = names.map(size);
  const raw = sizes.reduce((sum, each) => sum + each.raw, 0);
  const gzip = sizes.reduce((sum, each) => sum + each.gzip, 0);
  return `${names.length} ${names.length === 1 ? "file" : "files"}, ${raw} bytes, ${gzip} gzipped`;
}

/**
 * The report's text: a line for each kind with its first visit's and its deferred files' totals,
 * each file under it with its bytes (a deferred one marked `later`), then the build's every file
 * once. `size` answers a file's bytes by its path in the build.
 */
export function bundleReport(manifest: Manifest, size: (file: string) => Size): string {
  const all = kinds(manifest);
  const lines: string[] = [];
  const line = (mark: string, file: string) => {
    const { raw, gzip } = size(file);
    lines.push(`  ${mark}${file}  ${raw} bytes, ${gzip} gzipped`);
  };
  for (const { kind, first, later } of all) {
    const deferred = later.length > 0 ? `; later: ${total(later, size)}` : "";
    lines.push(`${kind}: first visit ${total(first, size)}${deferred}`);
    for (const file of first) line("", file);
    for (const file of later) line("later ", file);
  }
  const every = [...new Set(all.flatMap(({ first, later }) => [...first, ...later]))];
  lines.push(`the build: ${total(every, size)}`);
  return `${lines.join("\n")}\n`;
}
