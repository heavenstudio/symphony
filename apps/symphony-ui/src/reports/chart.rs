//! One chart of the Reports page, a period per column on the page's one time axis: bars as boxes
//! of the page (their width at most 24 px whatever the screen, their data end rounded, a 2 px gap of
//! the card between stacked parts), lines as one SVG over them whose strokes keep their width
//! however the plot stretches. A period no file recorded is hatched, never drawn as a zero, and a
//! line breaks across it. A tap or a key on a period shows every series' value in the chart's
//! readout, on every chart at once; the table under it says them all.

use dioxus::prelude::*;

/// How a series is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A bar from zero up, stacked on the bars before it.
    Bar,
    /// A bar from zero down, stacked under the ones before it: the other side of a diverging chart.
    Below,
    Line,
}

/// What a chart's figures count, which says how they are written and how its axis steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Things counted: tasks, commits, lines, requests.
    Count,
    /// A mean over the time watched: sessions at work.
    Mean,
    /// A duration in seconds.
    Seconds,
    /// Task-hours.
    Hours,
    /// The machine's load average.
    Load,
}

impl Unit {
    /// `value` as the chart writes it: `12`, `1.2k`, `2.25`, `45s`, `12m`, `3.5h`, `2.1d`.
    pub fn format(self, value: f64) -> String {
        match self {
            Unit::Count if value.abs() >= 1000.0 => format!("{}k", short(value / 1000.0, 1)),
            Unit::Count => short(value, 2),
            Unit::Mean | Unit::Load => short(value, 2),
            Unit::Hours => format!("{}h", short(value, 1)),
            Unit::Seconds => {
                let seconds = value.abs();
                if seconds < 60.0 {
                    format!("{}s", short(value, 0))
                } else if seconds < 3_600.0 {
                    format!("{}m", short(value / 60.0, u8::from(seconds < 600.0)))
                } else if seconds < 172_800.0 {
                    format!("{}h", short(value / 3_600.0, 1))
                } else {
                    format!("{}d", short(value / 86_400.0, 1))
                }
            }
        }
    }

    /// The smallest step its axis takes: a whole one of what is counted.
    fn least_step(self) -> f64 {
        match self {
            Unit::Count => 1.0,
            Unit::Seconds => 1.0,
            Unit::Mean | Unit::Load | Unit::Hours => 0.5,
        }
    }
}

/// `value` to at most `places` decimals, with no trailing zero: `2.5`, `3`, `0.25`; a value that
/// rounds to zero is `0`, never `-0` (an empty sum of floats is negative zero).
pub(crate) fn short(value: f64, places: u8) -> String {
    let text = format!("{value:.*}", usize::from(places));
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    if text == "-0" { "0" } else { text }.to_string()
}

/// `n` as a float: a count of periods or series, far below 2^32.
pub(crate) fn float(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// One series: its name, how it is drawn and its value in each period, none where its source has
/// no record of the period.
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesData {
    pub name: &'static str,
    pub mark: Mark,
    pub values: Vec<Option<f64>>,
}

/// One chart, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartData {
    pub id: &'static str,
    pub title: &'static str,
    /// What it shows and in what unit, a sentence.
    pub note: String,
    pub unit: Unit,
    /// Its series in their fixed order: the first takes `--chart-1`, the next `--chart-2`, and so on.
    pub series: Vec<SeriesData>,
    /// How much of each period its source recorded, from 0 to 1.
    pub cover: Vec<f64>,
    /// What it says in place of the plot when the window cannot show it (lead time by the hour).
    pub instead: Option<&'static str>,
}

impl ChartData {
    /// The value of `series` in period `at`.
    fn value(&self, series: usize, at: usize) -> Option<f64> {
        self.series.get(series)?.values.get(at).copied().flatten()
    }

    /// Whether no series has a value in period `at`: a gap.
    pub fn gap(&self, at: usize) -> bool {
        (0..self.series.len()).all(|series| self.value(series, at).is_none())
    }
}

/// The page's one time axis, which every chart shares.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    /// Each period as the readout and the table name it: `Mon 9 Nov`, `Week of 9 Nov`,
    /// `Mon 9 Nov, 14:00 UTC`.
    pub periods: Vec<String>,
    /// The hours of each period, for the readout's coverage.
    pub hours: Vec<f64>,
    /// The periods labelled under the plot, each with its label.
    pub ticks: Vec<(usize, String)>,
    /// The settings changes: where each stands across the plot (0 to 100), its period and what it
    /// says.
    pub marks: Vec<(f64, usize, String)>,
}

/// A chart's vertical scale: from `low` (zero, or below it for a diverging chart) to `high`, and
/// the ticks between, a whole step apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub low: f64,
    pub high: f64,
    pub ticks: Vec<f64>,
}

impl Scale {
    /// The scale that holds every period's stacked bars and every line of `chart`.
    pub fn of(chart: &ChartData) -> Scale {
        let periods = chart.series.first().map_or(0, |series| series.values.len());
        let (mut up, mut down) = (0.0_f64, 0.0_f64);
        for at in 0..periods {
            let mut bars = 0.0;
            let mut below = 0.0;
            for (series, data) in chart.series.iter().enumerate() {
                let value = chart.value(series, at).unwrap_or(0.0).max(0.0);
                match data.mark {
                    Mark::Bar => bars += value,
                    Mark::Below => below += value,
                    Mark::Line => up = up.max(value),
                }
            }
            up = up.max(bars);
            down = down.max(below);
        }
        if up + down <= 0.0 {
            up = chart.unit.least_step();
        }
        let step = step(chart.unit, (up + down) / 4.0);
        let high = (up / step).ceil() * step;
        // From zero, so a chart with nothing below its axis starts at 0, not -0.
        let low = 0.0 - (down / step).ceil() * step;
        let mut ticks = Vec::new();
        let mut tick = low;
        while tick <= high + step / 2.0 {
            ticks.push(tick);
            tick += step;
        }
        Scale { low, high, ticks }
    }

    /// Where `value` stands from the bottom of the plot, 0 to 100.
    pub fn at(&self, value: f64) -> f64 {
        (value - self.low) / (self.high - self.low) * 100.0
    }
}

/// The step an axis of `unit` takes for `raw`: 1, 2, 2.5 or 5 times a power of ten (a whole number
/// for a count), or a round duration for seconds, never less than the unit's least.
fn step(unit: Unit, raw: f64) -> f64 {
    const DURATIONS: [f64; 22] = [
        1.0,
        2.0,
        5.0,
        10.0,
        15.0,
        30.0,
        60.0,
        120.0,
        300.0,
        600.0,
        900.0,
        1_800.0,
        3_600.0,
        7_200.0,
        10_800.0,
        21_600.0,
        43_200.0,
        86_400.0,
        172_800.0,
        259_200.0,
        604_800.0,
        1_209_600.0,
    ];
    let raw = raw.max(unit.least_step());
    if unit == Unit::Seconds
        && let Some(round) = DURATIONS.iter().find(|round| **round >= raw)
    {
        return *round;
    }
    let magnitude = 10_f64.powf(raw.log10().floor());
    let fraction = raw / magnitude;
    // A count's axis steps by whole ones: 2.5 only where it is 25, 250….
    let nice = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .filter(|nice| unit != Unit::Count || (nice * magnitude).fract() == 0.0)
        .find(|nice| fraction <= *nice)
        .unwrap_or(10.0);
    (nice * magnitude).max(unit.least_step())
}

/// One part of a period's bar: its series, where it starts and how tall it is (0 to 100), whether
/// it goes up from zero or down, whether it is its stack's far end (rounded) and whether a part is
/// under it (a gap of the card between them).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub series: usize,
    pub bottom: f64,
    pub height: f64,
    pub up: bool,
    pub end: bool,
    pub after: bool,
}

/// The bar parts of period `at` of `chart` on `scale`.
pub fn segments(chart: &ChartData, at: usize, scale: &Scale) -> Vec<Segment> {
    let mut parts = Vec::new();
    for up in [true, false] {
        let mark = if up { Mark::Bar } else { Mark::Below };
        let mut base = 0.0;
        let first = parts.len();
        for (series, data) in chart.series.iter().enumerate() {
            let value = chart.value(series, at).unwrap_or(0.0);
            if data.mark != mark || value <= 0.0 {
                continue;
            }
            let (from, to) = if up {
                (base, base + value)
            } else {
                (-(base + value), -base)
            };
            parts.push(Segment {
                series,
                bottom: scale.at(from),
                height: scale.at(to) - scale.at(from),
                up,
                end: false,
                after: parts.len() > first,
            });
            base += value;
        }
        if let Some(last) = parts.get_mut(first..).and_then(<[Segment]>::last_mut) {
            last.end = true;
        }
    }
    parts
}

/// A line series across the periods on `scale`: each run of two periods or more with a value as
/// the points of a polyline (`x,y`, a period one unit wide, y down from the top, 0 to 100), and
/// each period with a value alone between gaps as a dot.
pub fn runs(values: &[Option<f64>], scale: &Scale) -> (Vec<String>, Vec<(f64, f64)>) {
    let mut lines = Vec::new();
    let mut dots = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut flush = |run: &mut Vec<(f64, f64)>| {
        match run.as_slice() {
            [] => {}
            [one] => dots.push(*one),
            points => lines.push(
                points
                    .iter()
                    .map(|(x, y)| format!("{x},{}", short(*y, 2)))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        }
        run.clear();
    };
    for (at, value) in values.iter().enumerate() {
        match value {
            Some(value) => run.push((float(at) + 0.5, 100.0 - scale.at(*value))),
            None => flush(&mut run),
        }
    }
    flush(&mut run);
    (lines, dots)
}

/// The period a chart shows in its readout: the one tapped, else the last with a value.
pub fn shown(chart: &ChartData, selected: Option<usize>, periods: usize) -> Option<usize> {
    selected
        .filter(|at| *at < periods)
        .or_else(|| (0..periods).rev().find(|at| !chart.gap(*at)))
}

/// `part` of `whole` hours as the readout says it, when the source recorded only part of the
/// period: `18 of 24 h recorded`; none for a whole period or none of it.
pub fn coverage(part: f64, whole: f64) -> Option<String> {
    let hours = part * whole;
    (part > 0.0 && part < 0.99).then(|| {
        format!(
            "{} of {} h recorded",
            short(hours, u8::from(hours < 10.0)),
            short(whole, 0)
        )
    })
}

#[component]
pub fn ChartCard(
    chart: ChartData,
    axis: Axis,
    selected: Option<usize>,
    onselect: EventHandler<usize>,
) -> Element {
    let mut table = use_signal(|| false);
    let periods = axis.periods.len();
    let title_id = format!("chart-{}-title", chart.id);
    let legend = chart.series.len() > 1;
    if let Some(instead) = chart.instead {
        return rsx! {
            figure { class: "chart", id: "chart-{chart.id}", "aria-labelledby": "{title_id}",
                figcaption { h3 { id: "{title_id}", "{chart.title}" } p { class: "chart-note", "{chart.note}" } }
                p { class: "chart-instead", "{instead}" }
            }
        };
    }
    let scale = Scale::of(&chart);
    let shown = shown(&chart, selected, periods);
    let pick = move |step: isize| {
        let next = match shown {
            Some(at) => at
                .saturating_add_signed(step)
                .min(periods.saturating_sub(1)),
            None => periods.saturating_sub(1),
        };
        onselect.call(next);
    };
    rsx! {
        figure { class: "chart", id: "chart-{chart.id}", "aria-labelledby": "{title_id}",
            figcaption {
                h3 { id: "{title_id}", "{chart.title}" }
                p { class: "chart-note", "{chart.note}" }
            }
            if legend {
                ul { class: "chart-legend",
                    for (series, data) in chart.series.iter().enumerate() {
                        li { key: "{data.name}",
                            span { class: if data.mark == Mark::Line { "swatch line" } else { "swatch" }, style: "--c:var(--chart-{series + 1})", "aria-hidden": "true" }
                            "{data.name}"
                        }
                    }
                }
            }
            {readout(&chart, &axis, shown)}
            div { class: "chart-plot", tabindex: "0", role: "group",
                "aria-label": "{chart.title}: the left and right arrow keys move between periods",
                onkeydown: move |event: KeyboardEvent| {
                    let step = match event.key() {
                        Key::ArrowLeft => -1,
                        Key::ArrowRight => 1,
                        Key::Home => isize::MIN,
                        Key::End => isize::MAX,
                        _ => return,
                    };
                    event.prevent_default();
                    pick(step);
                },
                div { class: "chart-y", "aria-hidden": "true",
                    for tick in scale.ticks.iter().copied() {
                        span { key: "{tick}", style: "bottom:{short(scale.at(tick), 2)}%", "{chart.unit.format(tick)}" }
                    }
                }
                div { class: "chart-area",
                    for tick in scale.ticks.iter().copied() {
                        div { key: "{tick}", class: if tick == 0.0 { "chart-grid zero" } else { "chart-grid" }, style: "bottom:{short(scale.at(tick), 2)}%" }
                    }
                    for (left, _, text) in axis.marks.iter() {
                        div { key: "{left}", class: "chart-mark", style: "left:{short(*left, 2)}%", title: "{text}" }
                    }
                    div { class: "chart-cols", style: "--n:{periods}",
                        for at in 0..periods {
                            div { key: "{at}",
                                class: format!("chart-col{}{}", if chart.gap(at) { " gap" } else { "" }, if shown == Some(at) { " on" } else { "" }),
                                ..crate::entry_events::click(move || onselect.call(at)),
                                for part in segments(&chart, at, &scale) {
                                    span { key: "{part.series}",
                                        class: format!("seg {}{}{}", if part.up { "up" } else { "down" }, if part.end { " end" } else { "" }, if part.after { " after" } else { "" }),
                                        style: "bottom:{short(part.bottom, 2)}%;height:{short(part.height, 2)}%;--c:var(--chart-{part.series + 1})",
                                    }
                                }
                            }
                        }
                    }
                    svg { class: "chart-lines", view_box: "0 0 {periods} 100", preserve_aspect_ratio: "none", "aria-hidden": "true",
                        for (series, data) in chart.series.iter().enumerate().filter(|(_, data)| data.mark == Mark::Line) {
                            {line_series(series, &data.values, &scale, shown)}
                        }
                    }
                }
            }
            div { class: "chart-x", "aria-hidden": "true",
                for (at, label) in axis.ticks.iter() {
                    span { key: "{at}", style: "left:{short((float(*at) + 0.5) / float(periods.max(1)) * 100.0, 2)}%", "{label}" }
                }
            }
            button { class: "chart-table-toggle", r#type: "button", "aria-expanded": table(),
                ..crate::entry_events::click(move || table.toggle()),
                if table() { "Hide the table" } else { "Show the table" }
            }
            if table() {
                {self::table(&chart, &axis)}
            }
        }
    }
}

/// A line series' strokes, its dots alone between gaps and the period shown ringed: one group of
/// the chart's SVG, keyed by its series. A function rather than a component, as are the readout,
/// the table and the ghost: each is drawn again whenever its chart is, so a component's own props
/// and memo would only add bytes to the module.
fn line_series(
    series: usize,
    values: &[Option<f64>],
    scale: &Scale,
    shown: Option<usize>,
) -> Element {
    let (lines, dots) = runs(values, scale);
    let colour = format!("var(--chart-{})", series + 1);
    let picked = shown.and_then(|at| {
        values
            .get(at)
            .copied()
            .flatten()
            .map(|value| (float(at) + 0.5, 100.0 - scale.at(value)))
    });
    rsx! {
        g { key: "{series}", stroke: "{colour}",
            for (n, points) in lines.iter().enumerate() {
                polyline { key: "{n}", points: "{points}" }
            }
            for (x, y) in dots {
                line { key: "{x}", class: "dot", x1: "{x}", y1: "{short(y, 2)}", x2: "{x}", y2: "{short(y, 2)}" }
            }
            if let Some((x, y)) = picked {
                line { class: "ring", x1: "{x}", y1: "{short(y, 2)}", x2: "{x}", y2: "{short(y, 2)}" }
                line { class: "dot", x1: "{x}", y1: "{short(y, 2)}", x2: "{x}", y2: "{short(y, 2)}" }
            }
        }
    }
}

/// The values of the period `at` shows, one line per series, in the text's own ink.
fn readout(chart: &ChartData, axis: &Axis, at: Option<usize>) -> Element {
    let Some(at) = at else {
        return rsx! { div { class: "chart-readout", "aria-live": "polite", p { class: "chart-when", "No period recorded yet" } } };
    };
    let period = axis.periods.get(at).cloned().unwrap_or_default();
    let recorded = chart
        .cover
        .get(at)
        .zip(axis.hours.get(at))
        .and_then(|(part, whole)| coverage(*part, *whole));
    let gap = chart.gap(at);
    rsx! {
        div { class: "chart-readout", "aria-live": "polite",
            p { class: "chart-when",
                strong { "{period}" }
                if gap { " · no data recorded" }
                if let Some(recorded) = recorded { " · {recorded}" }
            }
            if !gap {
                ul {
                    for (series, data) in chart.series.iter().enumerate() {
                        li { key: "{data.name}",
                            span { class: if data.mark == Mark::Line { "swatch line" } else { "swatch" }, style: "--c:var(--chart-{series + 1})", "aria-hidden": "true" }
                            "{data.name} "
                            strong { {chart.value(series, at).map_or_else(|| "no data".to_string(), |value| chart.unit.format(value))} }
                        }
                    }
                }
            }
            for (_, _, text) in axis.marks.iter().filter(|(_, period, _)| *period == at) {
                p { key: "{text}", class: "chart-mark-note", "Settings changed: {text}" }
            }
        }
    }
}

/// Every period's values, oldest first: what the chart draws, for a reader who reads it as text.
pub(crate) fn table(chart: &ChartData, axis: &Axis) -> Element {
    rsx! {
        div { class: "chart-table",
            table {
                caption { "{chart.title}, by period" }
                thead { tr {
                    th { scope: "col", "Period" }
                    for data in chart.series.iter() { th { key: "{data.name}", scope: "col", "{data.name}" } }
                } }
                tbody {
                    for (at, period) in axis.periods.iter().enumerate() {
                        tr { key: "{at}",
                            th { scope: "row", "{period}" }
                            for (series, data) in chart.series.iter().enumerate() {
                                td { key: "{data.name}", {chart.value(series, at).map_or_else(|| "–".to_string(), |value| chart.unit.format(value))} }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A chart still to come: its box, a bar for its title and note, and its plot's place.
pub fn ghost_chart() -> Element {
    rsx! {
        div { class: "ghost-chart", "aria-hidden": "true",
            span { class: "bone", style: "--w:40%" }
            span { class: "bone", style: "--w:70%" }
            div { class: "ghost-plot" }
        }
    }
}
