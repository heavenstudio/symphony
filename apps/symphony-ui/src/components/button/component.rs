use super::super::merge_attributes;
super::super::event_attributes!(ButtonPropsBuilder {
    onclick: MouseEvent,
    onmousedown: MouseEvent,
    onmouseup: MouseEvent,
    onkeydown: KeyboardEvent,
});
use dioxus::prelude::*;

const STYLE: Asset = asset!(
    "/src/components/button/style.css",
    AssetOptions::css().with_static_head(true)
);

#[derive(Copy, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Destructive,
    Outline,
    Ghost,
    Link,
}

impl ButtonVariant {
    pub fn class(&self) -> &'static str {
        match self {
            ButtonVariant::Primary => "primary",
            ButtonVariant::Secondary => "secondary",
            ButtonVariant::Destructive => "destructive",
            ButtonVariant::Outline => "outline",
            ButtonVariant::Ghost => "ghost",
            ButtonVariant::Link => "link",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum ButtonSize {
    Xs,
    Sm,
    #[default]
    Default,
    Lg,
    Icon,
    IconXs,
    IconSm,
    IconLg,
}

impl ButtonSize {
    pub fn class(&self) -> &'static str {
        match self {
            ButtonSize::Xs => "xs",
            ButtonSize::Sm => "sm",
            ButtonSize::Default => "default",
            ButtonSize::Lg => "lg",
            ButtonSize::Icon => "icon",
            ButtonSize::IconXs => "icon-xs",
            ButtonSize::IconSm => "icon-sm",
            ButtonSize::IconLg => "icon-lg",
        }
    }
}

#[component]
pub fn Button(
    #[props(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(extends=GlobalAttributes)]
    #[props(extends=button)]
    attributes: Vec<Attribute>,
    // Plain action listeners retain the entry views' compact event representation.
    #[props(default)] actions: Vec<Attribute>,
    children: Element,
) -> Element {
    std::hint::black_box(&STYLE);
    let base = vec![
        Attribute::new("class", "dx-button", None, false),
        Attribute::new("data-style", variant.class(), None, false),
        Attribute::new("data-size", size.class(), None, false),
    ];
    let merged = merge_attributes(vec![base, attributes, actions]);

    rsx! {
        button {
            ..merged,
            {children}
        }
    }
}
