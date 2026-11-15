// This source is included only by symphony-ui-dev. No release page loads it.
(() => {
    window.reviewPanelReady = true;
    window.reviewDevelopment = true;
    const held = [];
    const drafts = new Map();
    function engaged() {
        return [...drafts].some(([field, value]) => field.isConnected && value.trim()) ||
            document.querySelector('[data-busy=true],[data-posting=true],dialog[open],[role=dialog],[role=listbox],[role=menu],details[open],#quick-sheet:not([hidden]),[contenteditable=true]') ||
            document.activeElement?.matches('input,textarea,select,[contenteditable]');
    }
    document.addEventListener('input', event => {
        const field = event.target;
        if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) drafts.set(field, field.value);
    });
    // DX normally reloads immediately on a fallback rebuild or reconnect. Hold that event until
    // the same interaction gate as the live feed is clear; template and code patches pass through.
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
        constructor(url, protocols) {
            super(url, protocols);
            if (new URL(url, location.href).pathname !== '/_dioxus') return;
            let replaying = false;
            const defer = event => {
                if (replaying || !engaged()) return;
                event.stopImmediatePropagation();
                held.push(() => {
                    replaying = true;
                    this.dispatchEvent(new Event(event.type));
                    replaying = false;
                });
            };
            this.addEventListener('open', defer);
            this.addEventListener('message', event => {
                if (replaying || event.data !== '"FullReloadCommand"' || !engaged()) return;
                event.stopImmediatePropagation();
                held.push(() => {
                    replaying = true;
                    this.dispatchEvent(new MessageEvent('message', { data: event.data }));
                    replaying = false;
                });
            });
        }
    };
    setInterval(() => {
        for (const [field] of drafts) {
            if (!field.isConnected || !field.value.trim()) drafts.delete(field);
        }
        if (!engaged()) held.splice(0).forEach(replay => replay());
    }, 200);
    // The dev binary has every page kind in one runtime; the release shell owns this in production.
    document.addEventListener('click', event => {
        if (event.defaultPrevented || event.button || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        const link = event.target instanceof Element && event.target.closest('a[href]');
        if (!link || link.hasAttribute('download') || link.dataset.native === 'true' || (link.target && link.target !== '_self')) return;
        const url = new URL(link.href, location.href);
        if (url.origin !== location.origin || !window.reviewDevRoute?.(url.pathname)) return;
        event.preventDefault();
        if (document.querySelector('[data-posting=true]')) return;
        history.pushState(null, '', url);
        dispatchEvent(new Event('review:navigate'));
    });
    addEventListener('popstate', () => dispatchEvent(new Event('review:navigate')));
})();
