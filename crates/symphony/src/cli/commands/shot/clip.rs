//! Whether a box's `clip-path` leaves a point out, as a probe ([`super::AIM`]) read the box: its
//! computed `clip-path`, its border box where the page draws it, its borders, paddings and margins,
//! and how it and every box around it up the flat tree move.
//!
//! Only what the geometry backs is claimed. The shape is a basic shape (an inset, its corners
//! rounded or not, a circle, an ellipse or a polygon) or a reference box alone, its lengths in
//! pixels, percentages or `calc()`, `min()`, `max()` and `clamp()` of them, a position by its
//! keywords, its offsets from them or its lengths, on any of the box's reference boxes. The box,
//! and every box around it, moves only by translations, which move the box and its shape alike: a
//! box mirrored, turned, scaled, zoomed or on a motion path is never tested at the point its own
//! coordinates would put there. Anything else, a `path()`, a `url()` or what no reader here knows,
//! leaves the point in, so its cause is never named.

use serde_json::Value;

/// A box between the element and its ancestor on top at its middle, the element's own among them,
/// with a `clip-path`, as the probe read it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shaped {
    /// Its name, in its outermost host's when it lies in a shadow root.
    pub(super) name: String,
    /// Whether it is the element aimed at itself.
    pub(super) own: bool,
    /// Its `clip-path`, as `getComputedStyle` says it.
    clip: String,
    /// Its border box, in the page's coordinates.
    border_box: Rect,
    border: Sides,
    padding: Sides,
    margin: Sides,
    /// Whether it may scroll (an overflow of `auto` or `scroll`), so a scrollbar may stand between
    /// its border and its padding.
    scrolls: bool,
    /// How it and each box around it up the flat tree move, from it outwards.
    moves: Vec<Motion>,
}

/// A rectangle: its left and top edges, its width and its height, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

/// Four lengths in CSS pixels, by side: top, right, bottom, left.
type Sides = [f64; 4];

/// How a box moves, by its computed `transform`, `translate`, `scale`, `rotate`, `offset-path`
/// and `zoom`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Motion {
    pub(super) transform: String,
    pub(super) translate: String,
    pub(super) scale: String,
    pub(super) rotate: String,
    pub(super) offset_path: String,
    pub(super) zoom: String,
}

impl Shaped {
    /// One of a probe's `shapes`: `{name, own, clip, box, border, padding, margin, scrolls,
    /// moves}`, each motion `[transform, translate, scale, rotate, offsetPath, zoom]`. `None` for
    /// anything short of that.
    pub(super) fn read(value: &Value) -> Option<Shaped> {
        let number = |value: &Value| value.as_f64();
        let four = |key: &str| -> Option<[f64; 4]> {
            let all: Vec<f64> = value[key]
                .as_array()?
                .iter()
                .map(number)
                .collect::<Option<_>>()?;
            all.try_into().ok()
        };
        let [x, y, width, height] = four("box")?;
        let moves = value["moves"]
            .as_array()?
            .iter()
            .map(Motion::read)
            .collect::<Option<_>>()?;
        Some(Shaped {
            name: value["name"].as_str()?.to_string(),
            own: value["own"].as_bool()?,
            clip: value["clip"].as_str()?.to_string(),
            border_box: Rect {
                x,
                y,
                width,
                height,
            },
            border: four("border")?,
            padding: four("padding")?,
            margin: four("margin")?,
            scrolls: value["scrolls"].as_bool()?,
            moves,
        })
    }

    /// Whether its `clip-path` leaves `(x, y)`, in the page's coordinates, out: only where the
    /// shape, its reference box and the box's place are all read ([`self`](self)).
    pub(super) fn excludes(&self, (x, y): (f64, f64)) -> bool {
        if !self.moves.iter().all(Motion::still) {
            return false;
        }
        let Some((shape, reference)) = split(&self.clip) else {
            return false;
        };
        let Some(area) = self.reference(reference) else {
            return false;
        };
        cuts(shape, area, (x - area.x, y - area.y)).unwrap_or(false)
    }

    /// The reference box `name` names, in the page's coordinates: the border box (by default, and
    /// for a stroke or view box, as for any box with a CSS layout), the padding box, the content
    /// box (a fill box's too), the margin box. `None` for another name, or for the padding or
    /// content box of a box that may scroll, where a scrollbar may take room this does not read.
    fn reference(&self, name: &str) -> Option<Rect> {
        let inset = |by: Sides| Rect {
            x: self.border_box.x + by[3],
            y: self.border_box.y + by[0],
            width: self.border_box.width - by[1] - by[3],
            height: self.border_box.height - by[0] - by[2],
        };
        let added = |a: Sides, b: Sides| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
        match name {
            "" | "border-box" | "stroke-box" | "view-box" => Some(self.border_box),
            "padding-box" if !self.scrolls => Some(inset(self.border)),
            "content-box" | "fill-box" if !self.scrolls => {
                Some(inset(added(self.border, self.padding)))
            }
            "margin-box" => Some(inset(self.margin.map(|side| -side))),
            _ => None,
        }
    }
}

impl Motion {
    /// `[transform, translate, scale, rotate, offsetPath, zoom]`, each a string.
    fn read(value: &Value) -> Option<Motion> {
        let text = |at: usize| value.get(at)?.as_str().map(str::to_string);
        Some(Motion {
            transform: text(0)?,
            translate: text(1)?,
            scale: text(2)?,
            rotate: text(3)?,
            offset_path: text(4)?,
            zoom: text(5)?,
        })
    }

    /// Whether it moves the box only across the page, if at all: a transform whose matrix is a
    /// translation within the page's plane (`translateZ(0)` among them, which Chrome reads as no
    /// move), a `translate` with no depth, no `scale` or `rotate` but the identity, no motion path
    /// and no zoom. Its shape then moves with its box, where the page reads the box.
    pub(super) fn still(&self) -> bool {
        let translation = match self.transform.as_str() {
            "none" => true,
            transform => matrix(transform).is_some_and(|matrix| {
                matrix
                    .iter()
                    .enumerate()
                    .all(|(at, value)| [12, 13].contains(&at) || *value == IDENTITY[at])
            }),
        };
        let depthless = match self.translate.split_whitespace().collect::<Vec<_>>()[..] {
            [] | [_] | [_, _] => true,
            [_, _, depth] => length(depth, 0.0) == Some(0.0),
            _ => false,
        };
        let unscaled = self.scale == "none"
            || self
                .scale
                .split_whitespace()
                .all(|factor| factor.parse::<f64>() == Ok(1.0));
        let unturned = self.rotate == "none"
            || self
                .rotate
                .split_whitespace()
                .last()
                .and_then(angle)
                .is_some_and(|turn| turn == 0.0);
        translation
            && depthless
            && unscaled
            && unturned
            && self.offset_path == "none"
            && self.zoom.parse::<f64>() == Ok(1.0)
    }
}

/// The identity matrix, column by column, as `matrix3d()` lists it.
const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// A computed transform's matrix as `matrix3d()` lists it, column by column: `matrix(a, b, c, d,
/// e, f)` is `matrix3d(a, b, 0, 0, c, d, 0, 0, 0, 0, 1, 0, e, f, 0, 1)`.
fn matrix(transform: &str) -> Option<[f64; 16]> {
    let (name, values) = call(transform)?;
    let values: Vec<f64> = values
        .split(',')
        .map(|value| value.trim().parse::<f64>().ok())
        .collect::<Option<_>>()?;
    match (name, values.as_slice()) {
        ("matrix", &[a, b, c, d, e, f]) => Some([
            a, b, 0.0, 0.0, c, d, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, e, f, 0.0, 1.0,
        ]),
        ("matrix3d", _) => values.try_into().ok(),
        _ => None,
    }
}

/// An angle in turns: `0deg`, `0.5turn`, `3.14rad`, `200grad`.
fn angle(text: &str) -> Option<f64> {
    let (number, turns) = [
        ("deg", 360.0),
        ("grad", 400.0),
        ("rad", std::f64::consts::TAU),
        ("turn", 1.0),
    ]
    .into_iter()
    .find_map(|(unit, whole)| Some((text.strip_suffix(unit)?, whole)))?;
    Some(number.parse::<f64>().ok()? / turns)
}

/// A function call `name(args)` taking the whole of `text`: its name and what is between its
/// parentheses.
fn call(text: &str) -> Option<(&str, &str)> {
    let open = text.find('(')?;
    let close = matching(text, open)?;
    if close + 1 != text.len() {
        return None;
    }
    Some((text.get(..open)?, text.get(open + 1..close)?))
}

/// Where the parenthesis opened at `open` in `text` closes.
fn matching(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, character) in text.char_indices().skip_while(|(at, _)| *at < open) {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// A computed `clip-path` as its shape, `(name, arguments)` or `None` for a reference box alone,
/// and its reference box's name, empty for the default border box. `None` for anything else.
fn split(clip: &str) -> Option<(Option<(&str, &str)>, &str)> {
    let clip = clip.trim();
    if is_box(clip) {
        return Some((None, clip));
    }
    let open = clip.find('(')?;
    let close = matching(clip, open)?;
    let name = clip.get(..open)?;
    let reference = clip.get(close + 1..)?.trim();
    if !["inset", "circle", "ellipse", "polygon"].contains(&name)
        || !(reference.is_empty() || is_box(reference))
    {
        return None;
    }
    Some((Some((name, clip.get(open + 1..close)?)), reference))
}

/// Whether `name` names a reference box.
fn is_box(name: &str) -> bool {
    [
        "border-box",
        "padding-box",
        "content-box",
        "margin-box",
        "fill-box",
        "stroke-box",
        "view-box",
    ]
    .contains(&name)
}

/// Whether `shape` (`None` for its reference box alone) on the reference box `area` leaves out
/// `(x, y)`, from the box's corner; `None` when it is not read.
fn cuts(shape: Option<(&str, &str)>, area: Rect, (x, y): (f64, f64)) -> Option<bool> {
    let (w, h) = (area.width, area.height);
    let Some((name, arguments)) = shape else {
        // The box alone, its corners rounded as its border's: outside its rectangle is out, and
        // inside it a rounded corner is not read.
        return Some(x < 0.0 || x >= w || y < 0.0 || y >= h);
    };
    match name {
        "inset" => inset(arguments, (w, h), (x, y)),
        "circle" | "ellipse" => {
            let words = words(arguments);
            let at = words.iter().position(|word| *word == "at");
            let (radii, position) = match at {
                Some(at) => (words.get(..at)?, words.get(at + 1..)?),
                None => (&words[..], &[][..]),
            };
            let (cx, cy) = place(position, (w, h))?;
            // How far the centre is from each side: left, right, top and bottom.
            let (left, right, top, bottom) = (cx.abs(), (w - cx).abs(), cy.abs(), (h - cy).abs());
            if name == "circle" {
                let radius = match radii {
                    [] | ["closest-side"] => left.min(right).min(top).min(bottom),
                    ["farthest-side"] => left.max(right).max(top).max(bottom),
                    [word] => length(word, w.hypot(h) / std::f64::consts::SQRT_2)?,
                    _ => return None,
                };
                return Some((x - cx).hypot(y - cy) > radius);
            }
            let side = |word: &str, near: f64, far: f64, basis: f64| match word {
                "closest-side" => Some(near.min(far)),
                "farthest-side" => Some(near.max(far)),
                word => length(word, basis),
            };
            let (rx, ry) = match radii {
                [] => (left.min(right), top.min(bottom)),
                [across, down] => (side(across, left, right, w)?, side(down, top, bottom, h)?),
                _ => return None,
            };
            Some(((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) > 1.0)
        }
        "polygon" => polygon(arguments, (w, h), (x, y)),
        _ => None,
    }
}

/// An inset's rectangle on a `w` by `h` box, its corners rounded by its `round` radii (each made
/// smaller alike where two along a side would overlap, as a border's are): whether `(x, y)` is
/// outside it. An inset whose sides cross keeps nothing.
fn inset(arguments: &str, (w, h): (f64, f64), (x, y): (f64, f64)) -> Option<bool> {
    let words = words(arguments);
    let (sides, radii) = match words.iter().position(|word| *word == "round") {
        Some(at) => (words.get(..at)?, words.get(at + 1..)?),
        None => (&words[..], &[][..]),
    };
    let [top, right, bottom, left] = four(sides)?;
    let (left, top) = (length(left, w)?, length(top, h)?);
    let (right, bottom) = (w - length(right, w)?, h - length(bottom, h)?);
    if right <= left || bottom <= top {
        return Some(true);
    }
    if x < left || x >= right || y < top || y >= bottom {
        return Some(true);
    }
    if radii.is_empty() {
        return Some(false);
    }
    let (across, down) = match radii.iter().position(|word| *word == "/") {
        Some(at) => (radii.get(..at)?, radii.get(at + 1..)?),
        None => (radii, radii),
    };
    let (across, down) = (four(across)?, four(down)?);
    // Top left, top right, bottom right, bottom left: each corner's horizontal and vertical radius,
    // percentages of the box's width and height.
    let mut corners = [(0.0, 0.0); 4];
    for (corner, (rx, ry)) in corners.iter_mut().zip(across.iter().zip(down)) {
        *corner = (length(rx, w)?, length(ry, h)?);
    }
    let (width, height) = (right - left, bottom - top);
    let wide = (corners[0].0 + corners[1].0).max(corners[3].0 + corners[2].0);
    let tall = (corners[0].1 + corners[3].1).max(corners[1].1 + corners[2].1);
    let mut factor: f64 = 1.0;
    if wide > width {
        factor = factor.min(width / wide);
    }
    if tall > height {
        factor = factor.min(height / tall);
    }
    let [tl, tr, br, bl] = corners.map(|(rx, ry)| (rx * factor, ry * factor));
    // Out at a corner: past its ellipse's centre towards the corner on both axes (`towards`, as
    // signs), and outside the ellipse.
    let out = |(rx, ry): (f64, f64), (cx, cy): (f64, f64), (sx, sy): (f64, f64)| {
        rx > 0.0
            && ry > 0.0
            && (x - cx) * sx > 0.0
            && (y - cy) * sy > 0.0
            && ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) > 1.0
    };
    Some(
        out(tl, (left + tl.0, top + tl.1), (-1.0, -1.0))
            || out(tr, (right - tr.0, top + tr.1), (1.0, -1.0))
            || out(br, (right - br.0, bottom - br.1), (1.0, 1.0))
            || out(bl, (left + bl.0, bottom - bl.1), (-1.0, 1.0)),
    )
}

/// One to four values, top, right, bottom and left as a margin's are: one for all four, two for
/// top and bottom then right and left, three for top, right and left, then bottom.
fn four<'a>(values: &[&'a str]) -> Option<[&'a str; 4]> {
    match *values {
        [all] => Some([all; 4]),
        [vertical, horizontal] => Some([vertical, horizontal, vertical, horizontal]),
        [top, horizontal, bottom] => Some([top, horizontal, bottom, horizontal]),
        [top, right, bottom, left] => Some([top, right, bottom, left]),
        _ => None,
    }
}

/// A polygon on a `w` by `h` box, by its fill rule: whether `(x, y)` is outside it.
fn polygon(arguments: &str, (w, h): (f64, f64), (x, y): (f64, f64)) -> Option<bool> {
    let mut items = items(arguments);
    let evenodd = match items.first().copied() {
        Some("evenodd") => {
            items.remove(0);
            true
        }
        Some("nonzero") => {
            items.remove(0);
            false
        }
        _ => false,
    };
    let points: Vec<(f64, f64)> = items
        .iter()
        .map(|item| match words(item)[..] {
            [px, py] => Some((length(px, w)?, length(py, h)?)),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if points.len() < 3 {
        return None;
    }
    let (mut crossings, mut winding) = (0_u32, 0_i32);
    for (at, &(x1, y1)) in points.iter().enumerate() {
        let (x2, y2) = points[(at + 1) % points.len()];
        if (y1 <= y) != (y2 <= y) && x1 + (y - y1) / (y2 - y1) * (x2 - x1) > x {
            crossings += 1;
            winding += if y2 > y1 { 1 } else { -1 };
        }
    }
    Some(if evenodd {
        crossings % 2 == 0
    } else {
        winding == 0
    })
}

/// A shape's centre on a `w` by `h` box from its `at` words: none for the middle; one, a keyword
/// or a length across, the other axis at its middle; two, across then down, keywords in either
/// order; four, each edge's keyword and the offset from it.
fn place(words: &[&str], (w, h): (f64, f64)) -> Option<(f64, f64)> {
    let across = |word: &str| match word {
        "left" => Some(0.0),
        "center" => Some(w / 2.0),
        "right" => Some(w),
        word => length(word, w),
    };
    let down = |word: &str| match word {
        "top" => Some(0.0),
        "center" => Some(h / 2.0),
        "bottom" => Some(h),
        word => length(word, h),
    };
    let vertical = |word: &str| word == "top" || word == "bottom";
    let horizontal = |word: &str| word == "left" || word == "right";
    match *words {
        [] => Some((w / 2.0, h / 2.0)),
        [one] if vertical(one) => Some((w / 2.0, down(one)?)),
        [one] => Some((across(one)?, h / 2.0)),
        [first, second] if vertical(first) || horizontal(second) => {
            Some((across(second)?, down(first)?))
        }
        [first, second] => Some((across(first)?, down(second)?)),
        [edge, offset, other, by] => {
            let (edge, offset, other, by) = if vertical(edge) {
                (other, by, edge, offset)
            } else {
                (edge, offset, other, by)
            };
            let x = match edge {
                "left" => length(offset, w)?,
                "right" => w - length(offset, w)?,
                _ => return None,
            };
            let y = match other {
                "top" => length(by, h)?,
                "bottom" => h - length(by, h)?,
                _ => return None,
            };
            Some((x, y))
        }
        _ => None,
    }
}

/// `text` split at its whitespace outside parentheses.
fn words(text: &str) -> Vec<&str> {
    pieces(text, char::is_whitespace)
}

/// `text` split at its commas outside parentheses, each piece trimmed.
fn items(text: &str) -> Vec<&str> {
    pieces(text, |character| character == ',')
}

/// The non-empty pieces of `text` between the characters `apart` takes, outside parentheses.
fn pieces(text: &str, apart: impl Fn(char) -> bool) -> Vec<&str> {
    let mut found = Vec::new();
    let (mut depth, mut start) = (0_i32, 0);
    for (at, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 && apart(character) => {
                found.extend(text.get(start..at).map(str::trim));
                start = at + character.len_utf8();
            }
            _ => {}
        }
    }
    found.extend(text.get(start..).map(str::trim));
    found.retain(|piece| !piece.is_empty());
    found
}

/// A computed `<length-percentage>` in CSS pixels, a percentage of `basis`: `12px`, `25%`, `0`, or
/// `calc()`, `min()`, `max()` and `clamp()` of them (sums, differences, and products or quotients by
/// a number). `None` for anything else: another unit, a keyword, a length times a length.
pub(super) fn length(text: &str, basis: f64) -> Option<f64> {
    let mut reader = Reader {
        text: text.trim().as_bytes(),
        at: 0,
        basis,
    };
    let value = reader.sum()?;
    if reader.at != reader.text.len() {
        return None;
    }
    match value {
        Quantity::Length(length) => Some(length),
        Quantity::Number(0.0) => Some(0.0),
        Quantity::Number(_) => None,
    }
}

/// A value inside a computed length: a plain number, or a length in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Quantity {
    Number(f64),
    Length(f64),
}

/// Reads a computed length, its percentages of `basis`.
struct Reader<'a> {
    text: &'a [u8],
    at: usize,
    basis: f64,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn spaces(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    /// Terms joined by ` + ` and ` - `, a space on either side as `calc()` asks.
    fn sum(&mut self) -> Option<Quantity> {
        let mut total = self.product()?;
        loop {
            let before = self.at;
            self.spaces();
            let sign = match self.peek() {
                Some(b'+') => 1.0,
                Some(b'-') => -1.0,
                _ => {
                    self.at = before;
                    return Some(total);
                }
            };
            self.at += 1;
            if !self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                return None;
            }
            self.spaces();
            total = match (total, self.product()?) {
                (Quantity::Length(a), Quantity::Length(b)) => Quantity::Length(a + sign * b),
                (Quantity::Number(a), Quantity::Number(b)) => Quantity::Number(a + sign * b),
                _ => return None,
            };
        }
    }

    /// Values joined by `*` and `/`, one side a number.
    fn product(&mut self) -> Option<Quantity> {
        let mut total = self.value()?;
        loop {
            let before = self.at;
            self.spaces();
            let times = match self.peek() {
                Some(b'*') => true,
                Some(b'/') => false,
                _ => {
                    self.at = before;
                    return Some(total);
                }
            };
            self.at += 1;
            self.spaces();
            total = match (total, self.value()?, times) {
                (Quantity::Number(a), Quantity::Number(b), true) => Quantity::Number(a * b),
                (Quantity::Number(a), Quantity::Length(b), true)
                | (Quantity::Length(b), Quantity::Number(a), true) => Quantity::Length(a * b),
                (Quantity::Number(a), Quantity::Number(b), false) if b != 0.0 => {
                    Quantity::Number(a / b)
                }
                (Quantity::Length(a), Quantity::Number(b), false) if b != 0.0 => {
                    Quantity::Length(a / b)
                }
                _ => return None,
            };
        }
    }

    /// A number with its unit, a parenthesised sum, or `calc()`, `min()`, `max()` or `clamp()`.
    fn value(&mut self) -> Option<Quantity> {
        let rest = self.text.get(self.at..)?;
        for (name, pick) in [
            ("calc(", Pick::Only),
            ("min(", Pick::Least),
            ("max(", Pick::Most),
            ("clamp(", Pick::Clamp),
            ("(", Pick::Only),
        ] {
            if rest.starts_with(name.as_bytes()) {
                self.at += name.len();
                return self.arguments(pick);
            }
        }
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            self.at += 1;
        }
        if matches!(self.peek(), Some(b'e' | b'E'))
            && self
                .text
                .get(self.at + 1)
                .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
        {
            self.at += 2;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let number: f64 = std::str::from_utf8(self.text.get(start..self.at)?)
            .ok()?
            .parse()
            .ok()?;
        let rest = self.text.get(self.at..)?;
        if rest.starts_with(b"px") {
            self.at += 2;
            Some(Quantity::Length(number))
        } else if rest.starts_with(b"%") {
            self.at += 1;
            Some(Quantity::Length(number / 100.0 * self.basis))
        } else if rest.first().is_some_and(u8::is_ascii_alphabetic) {
            None
        } else {
            Some(Quantity::Number(number))
        }
    }

    /// The comma-separated sums of a function called after its opening parenthesis, through its
    /// closing one, as `pick` takes them.
    fn arguments(&mut self, pick: Pick) -> Option<Quantity> {
        let mut values = Vec::new();
        loop {
            self.spaces();
            values.push(self.sum()?);
            self.spaces();
            match self.peek() {
                Some(b',') if pick != Pick::Only => self.at += 1,
                Some(b')') => {
                    self.at += 1;
                    break;
                }
                _ => return None,
            }
        }
        let lengths = || {
            values
                .iter()
                .map(|value| match value {
                    Quantity::Length(length) => Some(*length),
                    Quantity::Number(_) => None,
                })
                .collect::<Option<Vec<f64>>>()
        };
        match (pick, &values[..]) {
            (Pick::Only, [one]) => Some(*one),
            (Pick::Least, _) => Some(Quantity::Length(lengths()?.into_iter().reduce(f64::min)?)),
            (Pick::Most, _) => Some(Quantity::Length(lengths()?.into_iter().reduce(f64::max)?)),
            (Pick::Clamp, [_, _, _]) => match lengths()?[..] {
                [least, value, most] => Some(Quantity::Length(value.min(most).max(least))),
                _ => None,
            },
            _ => None,
        }
    }
}

/// What a function inside a length makes of its arguments.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    /// Its one argument: `calc()` or parentheses.
    Only,
    Least,
    Most,
    /// The middle one, held between the first and the last.
    Clamp,
}

#[cfg(test)]
mod tests;
