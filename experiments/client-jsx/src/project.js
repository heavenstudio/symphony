// The project as the server inlines it in every page (`symphony_api::ProjectView`, the script
// element `symphony-project`), and the paths under its base.

const element = document.getElementById("symphony-project");

/** @type {{name: string, register: string, guide?: string, nav: Array<{href: string, label: string, document: string}>, entrypoints: string[], base?: string}} */
export const project = (() => {
  try {
    return { nav: [], entrypoints: [], base: "", ...JSON.parse(element.textContent) };
  } catch {
    return { name: "Symphony", register: "docs/DECISIONS.md", nav: [], entrypoints: [], base: "" };
  }
})();

/** `path` under the project's base: a site path takes it; anything else stays. */
export function href(path) {
  if (!project.base || !path.startsWith("/") || path.startsWith("//")) return path;
  return project.base + path;
}

/** The browser's path without the project's base. */
export function strip(path) {
  if (!project.base) return path;
  if (path === project.base) return "/";
  return path.startsWith(project.base + "/") ? path.slice(project.base.length) : path;
}

/** A key for what this browser keeps, the project's id first (`project::stored`). */
export function stored(key) {
  return project.base ? `${project.base.slice(1)}:${key}` : key;
}

/** The documents' entrypoints, the convention's `docs` when none is named. */
export function entrypoints() {
  return project.entrypoints.length ? project.entrypoints : ["docs"];
}

export function documentsHref() {
  const first = entrypoints()[0];
  return first ? `/${first}/` : "/";
}
