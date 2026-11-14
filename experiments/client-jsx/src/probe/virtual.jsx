// TanStack Virtual for a long column: 2,000 cards of uneven heights in a scrolling box, only those
// near the view in the DOM, each measured once drawn. The board pages its columns with Load more
// and the document draws a section as it nears the screen; a virtual list is what a port would
// reach for to keep a long column or thread short in the DOM. Scrolling runs the library's
// `flushSync` path.
import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

const CARDS = Array.from({ length: 2000 }, (_, i) => ({ id: i, lines: 1 + (i % 4) }));

export default function Virtual() {
  const box = useRef(null);
  const rows = useVirtualizer({
    count: CARDS.length,
    getScrollElement: () => box.current,
    estimateSize: () => 60,
    overscan: 4,
  });
  return (
    <section id="virtual" data-ready="true">
      <div id="virtual-column" ref={box} style={{ height: 600, overflow: "auto" }}>
        <ol style={{ height: rows.getTotalSize(), position: "relative", margin: 0, padding: 0, listStyle: "none" }}>
          {rows.getVirtualItems().map((row) => (
            <li
              key={row.key}
              data-index={row.index}
              ref={rows.measureElement}
              style={{ position: "absolute", top: 0, left: 0, right: 0, transform: `translateY(${row.start}px)` }}
            >
              {Array.from({ length: CARDS[row.index].lines }, (_, line) => (
                <p key={line} style={{ margin: 0, height: 24 }}>
                  task {CARDS[row.index].id}, line {line + 1}
                </p>
              ))}
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}
