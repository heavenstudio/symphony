//! The stylesheets every page shares, as `dx` builds them (apps/symphony-ui/bundle.sh): this binary
//! draws nothing and no page loads it. dx links each style it holds from the head of the page it
//! builds (`with_static_head`), minified and named by its bytes, and package.sh joins those into the
//! one shared stylesheet the web client's kinds take first: the frame's, the controls' theme, the
//! board's, and the shared controls' (a button, an input, a text area, a select's trigger), whose
//! class names the web client's markup uses.

#![forbid(unsafe_code)]

use dioxus::prelude::*;

const FRAME: Asset = asset!(
    "/assets/frame.css",
    AssetOptions::css().with_static_head(true)
);
const COMPONENT_THEME: Asset = asset!(
    "/assets/dx-components-theme.css",
    AssetOptions::css().with_static_head(true)
);
const BOARD: Asset = asset!(
    "/assets/board.css",
    AssetOptions::css().with_static_head(true)
);
const BUTTON: Asset = asset!(
    "/src/components/button/style.css",
    AssetOptions::css().with_static_head(true)
);
const INPUT: Asset = asset!(
    "/src/components/input/style.css",
    AssetOptions::css().with_static_head(true)
);
const TEXTAREA: Asset = asset!(
    "/src/components/textarea/style.css",
    AssetOptions::css().with_static_head(true)
);
const SELECT_TRIGGER: Asset = asset!(
    "/src/components/select/trigger.css",
    AssetOptions::css().with_static_head(true)
);

fn main() {
    // Kept in the linked binary, which is where dx finds the styles it puts in the head.
    std::hint::black_box((
        &FRAME,
        &COMPONENT_THEME,
        &BOARD,
        &BUTTON,
        &INPUT,
        &TEXTAREA,
        &SELECT_TRIGGER,
    ));
}
