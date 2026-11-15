// The overlay module (the shell's persistent kind `overlay`, mounted in `#overlay-root` on the
// first select or picture of any page): it draws what a page asks of it through the shell's
// `reviewOverlay` (select.tsx, media-layer.ts), a select's popup or the picture viewer. A request is
// data and one callback, left on the window (`reviewOverlayRequest`) with `review:overlay` said:
// the popup answers with the value chosen, or nothing once it closes; the viewer with a direction,
// 0 to close. A request of another owner replaces the one drawn, drawn afresh.

import { type Photo, PictureViewer } from "./picture-viewer";
import { SelectPopup } from "./select-popup";
import { field, list, snapshot, useWindowEvents } from "./shared";

const OVERLAY_EVENTS = ["review:overlay"] as const;

/** What a page asks the overlay to draw, as it left it on the window. */
export type Drawn =
  | {
      kind: "select";
      owner: string;
      id: string;
      label: string;
      value: string;
      options: [string, string][];
    }
  | { kind: "picture"; owner: string; photos: Photo[]; at: number }
  | null;

const text = (value: unknown) => (typeof value === "string" ? value : "");

/** The request `value` as the overlay draws it: what it names of its kind, each of its type; none
 * for one of no kind it draws. */
export function drawn(value: unknown): Drawn {
  const kind = field(value, "kind");
  const owner = text(field(value, "owner"));
  if (kind === "select") {
    const options = list(field(value, "options")).flatMap((option): [string, string][] =>
      Array.isArray(option) && typeof option[0] === "string" && typeof option[1] === "string"
        ? [[option[0], option[1]]]
        : [],
    );
    return {
      kind,
      owner,
      id: text(field(value, "id")),
      label: text(field(value, "label")),
      value: text(field(value, "value")),
      options,
    };
  }
  if (kind === "picture") {
    const photos = list(field(value, "photos")).map((photo) => ({
      src: text(field(photo, "src")),
      caption: text(field(photo, "caption")),
    }));
    const at = field(value, "at");
    return {
      kind,
      owner,
      photos,
      at: typeof at === "number" && Number.isFinite(at) ? Math.max(0, Math.trunc(at)) : 0,
    };
  }
  return null;
}

/** The request's callback called with `value`, when it has one. */
export function answer(request: unknown, value: unknown): void {
  const callback = field(request, "callback");
  if (typeof callback === "function") callback(value);
}

/** The overlay's host: the request on the window, drawn again each time the shell says it moved. */
export function OverlayHost() {
  useWindowEvents(OVERLAY_EVENTS);
  const request = snapshot("reviewOverlayRequest");
  const shown = drawn(request);
  if (shown?.kind === "select") {
    return (
      <SelectPopup
        key={shown.owner}
        id={shown.id}
        label={shown.label}
        value={shown.value}
        options={shown.options}
        onChange={(value) => answer(request, value)}
        onClose={() => answer(request, null)}
      />
    );
  }
  if (shown?.kind === "picture") {
    return (
      <PictureViewer
        key={shown.owner}
        photos={shown.photos}
        at={shown.at}
        command={(direction) => answer(request, direction)}
      />
    );
  }
  return null;
}
