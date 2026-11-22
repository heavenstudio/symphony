// Run in every page the app shows, before the page's own scripts: the app tells the owner of what
// waits on them itself, from every server with its windows closed, so a page's own notice would be
// a second one. Notifications always read as denied here, so the page never asks for them and never
// shows one, and a page that does not check before showing one is refused as an unsupported
// browser refuses it.
//
// The app puts this file in the page as it is (lib.rs, `include_str!`), with no build between, so
// it is TypeScript with no syntax of its own: every type is inferred, the answer's from the DOM's
// own `Notification.requestPermission` it is assigned to, and the client's `tsc` checks them
// (apps/symphony-web/tsconfig.json). A test there holds it to plain JavaScript.
(() => {
  const denied = "denied";
  function Notification() {
    throw new TypeError("Notifications are the Symphony app's own");
  }
  Object.defineProperty(Notification, "permission", { get: () => denied });
  Object.defineProperty(window, "Notification", {
    value: Notification,
    writable: true,
    configurable: true,
  });
  window.Notification.requestPermission = (answer) => {
    if (typeof answer === "function") answer(denied);
    return Promise.resolve(denied);
  };
})();
