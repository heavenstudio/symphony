use dioxus::prelude::*;
super::super::event_attributes!(TextareaPropsBuilder {
    oninput: FormEvent,
    onkeydown: KeyboardEvent,
    onmounted: MountedEvent
});

const STYLE: Asset = asset!(
    "/src/components/textarea/style.css",
    AssetOptions::css().with_static_head(true)
);

#[derive(Copy, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum TextareaVariant {
    #[default]
    Default,
    Fade,
    Outline,
    Ghost,
}

impl TextareaVariant {
    pub fn class(&self) -> &'static str {
        match self {
            TextareaVariant::Default => "default",
            TextareaVariant::Fade => "fade",
            TextareaVariant::Outline => "outline",
            TextareaVariant::Ghost => "ghost",
        }
    }
}

#[component]
pub fn Textarea(
    #[props(default)] variant: TextareaVariant,
    #[props(extends=GlobalAttributes)]
    #[props(extends=textarea)]
    attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    std::hint::black_box(&STYLE);
    rsx! {
        textarea {
            class: "dx-textarea",
            "data-slot": "textarea",
            "data-style": variant.class(),
            ..attributes,
            {children}
        }
    }
}
