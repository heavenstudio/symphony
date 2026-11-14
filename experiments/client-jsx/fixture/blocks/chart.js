// A project's own block, as a project would ship it: a bar chart from plain JSON,
// `{"title": "…", "unit": "kB", "bars": [{"label": "…", "value": 1.5}]}`, drawn with DOM calls only,
// so every label reaches the page as text, never as markup. The experiment's client draws it in a
// sandboxed frame or in the page itself through the same `draw(element, data)` (src/custom.jsx);
// its colours and type are the page's theme tokens, which the frame is handed.
const SVG = "http://www.w3.org/2000/svg";
const WIDTH = 640;
const LABEL = 150;
const ROW = 26;

function svg(name, attributes, text) {
  const element = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, String(value));
  if (text != null) element.textContent = text;
  return element;
}

export default function draw(element, data) {
  const bars = (Array.isArray(data?.bars) ? data.bars : []).filter((bar) => Number.isFinite(Number(bar?.value)));
  const unit = typeof data?.unit === "string" ? data.unit : "";
  const title = typeof data?.title === "string" ? data.title : "Chart";
  const most = Math.max(0, ...bars.map((bar) => Number(bar.value))) || 1;
  const height = bars.length * ROW + 8;
  const said = (bar) => `${Number(bar.value)} ${unit}`.trim();

  const figure = document.createElement("figure");
  figure.style.cssText = "margin:0;padding:12px 0;color:var(--foreground,#1f2328);font-family:var(--font-sans,system-ui,sans-serif)";
  const chart = svg("svg", { viewBox: `0 0 ${WIDTH} ${height}`, width: "100%", role: "img", "aria-label": `${title}: ${bars.map((bar) => `${bar.label} ${said(bar)}`).join(", ")}` });
  chart.style.cssText = `display:block;max-width:${WIDTH}px;height:auto;overflow:visible`;
  bars.forEach((bar, index) => {
    const y = 4 + index * ROW;
    const width = Math.max(1, ((WIDTH - LABEL - 90) * Number(bar.value)) / most);
    const name = svg("text", { x: LABEL - 10, y: y + 15, "text-anchor": "end", "font-size": 13 }, String(bar.label ?? ""));
    name.style.fill = "currentColor";
    const box = svg("rect", { x: LABEL, y: y + 3, width, height: ROW - 8, rx: 3 });
    box.style.fill = bar.accent ? "var(--muted-foreground,#6b7280)" : "var(--primary,#2a78d6)";
    const value = svg("text", { x: LABEL + width + 8, y: y + 15, "font-size": 12 }, said(bar));
    value.style.fill = "var(--muted-foreground,#6b7280)";
    chart.append(name, box, value);
  });
  const caption = document.createElement("figcaption");
  caption.textContent = title;
  caption.style.cssText = "margin-top:6px;font-size:13px;color:var(--muted-foreground,#6b7280)";
  figure.append(chart, caption);
  element.replaceChildren(figure);
  return { bars: bars.length };
}
