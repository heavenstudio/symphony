// A probe, not a block anyone would ship: what a project's module reaches from where the page draws
// it (src/custom.jsx, in a sandboxed frame or, with `?blocks=page`, in the page). Each check is
// tried and drawn as a row: reached or not, and what came back or why not. In the frame, the
// requests its policy refused are listed as the browser reported them. Its data names the read to
// try and the card an owner's approve is posted to:
// `{"read": "/api/board", "write": {"path": "docs/DECISIONS.md", "slug": "bench-card-12"}}`.
// It writes: run it on a scratch board only (README, "A project's own block").
export default async function draw(element, data) {
  const refused = [];
  document.addEventListener("securitypolicyviolation", (event) => refused.push(`${event.effectiveDirective} ${event.blockedURI}`));
  const framed = window.top !== window;
  const checks = [];
  const tried = async (name, attempt) => {
    try {
      checks.push({ name, reached: true, detail: String(await attempt()) });
    } catch (error) {
      checks.push({ name, reached: false, detail: `${error?.name ?? "Error"}: ${error?.message ?? error}` });
    }
  };

  await tried("the page around it", () => {
    const page = window.top.document;
    return `read "${page.title}", ${page.querySelectorAll("section, .doc-section").length} sections, ${page.body.innerText.length} characters`;
  });
  await tried("a function of the page replaced", () => {
    const own = window.top.fetch;
    window.top.fetch = (...args) => own(...args);
    window.top.fetch = own;
    return "its fetch replaced, then put back";
  });
  await tried("the API, read", async () => {
    const response = await fetch(data.read);
    const text = await response.text();
    return `${data.read}: ${response.status}, ${text.length} bytes`;
  });
  await tried("the API, written as the owner", async () => {
    const note = "Approved by a project's module, not by the owner (the plugins probe).";
    const response = await fetch("/api/decision", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: data.write.path, slug: data.write.slug, text: note, state: "approved", note, tap: `probe-${Date.now()}` }),
    });
    // Read whole, as a body left unread keeps the request in flight in Chrome's eyes.
    const answer = await response.text();
    if (!response.ok) throw new Error(`${response.status} ${answer}`);
    return `approve on ${data.write.slug}: ${response.status} ${answer}`;
  });
  await tried("localStorage", () => {
    localStorage.setItem("plugins-probe", "1");
    localStorage.removeItem("plugins-probe");
    return `${localStorage.length} keys read, one written and removed`;
  });
  await tried("IndexedDB", () =>
    new Promise((resolve, reject) => {
      const opening = indexedDB.open("plugins-probe");
      opening.onsuccess = () => {
        opening.result.close();
        indexedDB.deleteDatabase("plugins-probe");
        resolve("a database opened, then deleted");
      };
      opening.onerror = () => reject(opening.error);
    }),
  );
  await tried("cookies", () => `${document.cookie.length} characters read`);
  if (framed) {
    await tried("the page navigated", () => {
      window.top.location.href = "about:blank";
      return "navigated";
    });
  }
  // A refused request is reported a moment after it fails.
  await new Promise((resolve) => setTimeout(resolve, 50));
  if (framed) checks.push({ name: "refused by the frame's policy", reached: false, detail: refused.join("; ") || "nothing" });

  const table = document.createElement("table");
  table.style.cssText = "border-collapse:collapse;font-size:13px;width:100%";
  const row = (cells, head) => {
    const tr = document.createElement("tr");
    for (const text of cells) {
      const cell = document.createElement(head ? "th" : "td");
      cell.textContent = text;
      cell.style.cssText = "text-align:left;padding:4px 8px;border-bottom:1px solid var(--border,#ddd);vertical-align:top";
      tr.append(cell);
    }
    return tr;
  };
  table.append(row([framed ? "From a sandboxed frame" : "From the page itself", "Reached", "What came back"], true));
  for (const check of checks) table.append(row([check.name, check.reached ? "yes" : "no", check.detail]));
  element.replaceChildren(table);
  return checks;
}
