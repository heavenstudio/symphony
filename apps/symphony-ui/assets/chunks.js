// The program's chunk loader (src/split.rs), which package.sh minifies and appends to the program's
// script with its kind-to-chunk map in place of the placeholder below. `reviewChunk(kind)` fetches
// the kind's chunk once and instantiates it against the main module's exports, as dx's splitter's
// own loader does (wasm-split-cli 0.7.10, `makeLoad`), whose names the chunk imports. Unlike that
// loader, it rejects when the fetch fails, its answer is not ok or the instantiation fails, and
// forgets the failure: the kind's `run` rejects, so the shell ends in its `gone` state, and a retry
// fetches again.
globalThis.reviewChunk = (() => {
    const chunks = __CHUNKS__, fused = {}, loads = {};
    return kind => loads[kind] ||= (async () => {
        const response = await fetch(chunks[kind]);
        if (!response.ok) throw new Error(`${response.status} ${response.url}`);
        const main = __reviewInitSync();
        const { instance } = await WebAssembly.instantiateStreaming(response, {
            env: { memory: main.memory },
            __wasm_split: { ...main, ...fused },
        });
        Object.assign(fused, instance.exports);
    })().catch(error => {
        delete loads[kind];
        throw error;
    });
})();
