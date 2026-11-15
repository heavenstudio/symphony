// Run in every page the app shows, before the page's own scripts: the app tells the owner of what
// waits on them itself, from every server with its windows closed, so a page's own notice would be
// a second one. Notifications always read as denied here, so the page never asks for them and never
// shows one, and a page that does not check before showing one is refused as an unsupported
// browser refuses it.
(() => {
  const denied = "denied";
  function Notification() {
    throw new TypeError("Notifications are the Symphony app's own");
  }
  Object.defineProperty(Notification, "permission", { get: () => denied });
  Notification.requestPermission = (answer) => {
    if (typeof answer === "function") answer(denied);
    return Promise.resolve(denied);
  };
  Object.defineProperty(window, "Notification", {
    value: Notification,
    writable: true,
    configurable: true,
  });
})();
