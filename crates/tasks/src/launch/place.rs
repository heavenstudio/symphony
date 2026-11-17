//! Where a launched terminal shows in the Herdr app: a stream tab (`code`, `review`, `3d`, `infra`)
//! of the project's space, found by name and made when missing, given as the terminal's
//! `herdr.space` and `herdr.tab` labels.

use super::hc::{Hc, Object, text_of};
use super::{LaunchError, MAX_PANES, new_uuid};
use serde_json::Value;

/// The Herdr app tab (a stream of work) a role's terminal goes to.
pub(super) fn stream(role: &str) -> &'static str {
    if role == "reviewer" { "review" } else { "code" }
}

/// Where a terminal shows: a space and a tab of the Herdr app, and the names those the launch made
/// still need (`space` or `tab`, its id, its name), given once the app has taken them in.
pub(super) struct Place {
    pub(super) space: String,
    pub(super) tab: String,
    pub(super) unnamed: Vec<(&'static str, String, String)>,
}

/// The tabs of `stream` (`code`, `code-2`, ...) with their numbers, in order.
fn stream_tabs<'layout>(tabs: &'layout [Object], stream: &str) -> Vec<(u64, &'layout Object)> {
    let mut found: Vec<(u64, &Object)> = tabs
        .iter()
        .filter_map(|tab| {
            let label = text_of(tab, "name");
            if label == stream {
                return Some((1, tab));
            }
            let number = label.strip_prefix(stream)?.strip_prefix('-')?;
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((number.parse().ok()?, tab))
        })
        .collect();
    crate::order::sort_by_key(&mut found, &|(number, _)| *number);
    found
}

/// The project's space (found by its name, never by a remembered id; a new one when there is
/// none), and in it the first tab of `stream` with room, else the stream's next tab. Only a tab
/// named for the stream is a destination: the owner's tabs never are.
pub(super) fn place(hc: &dyn Hc, project: &str, stream: &str) -> Result<Place, LaunchError> {
    let spaces = hc
        .layout()
        .map_err(|e| LaunchError(format!("cannot find the Herdr app's spaces: {e}")))?;
    let mut unnamed = Vec::new();
    let found = spaces.iter().find(|s| text_of(s, "name") == project);
    let (space, tabs) = match found {
        Some(space) => (
            text_of(space, "id"),
            space
                .get("tabs")
                .and_then(Value::as_array)
                .map(|tabs| tabs.iter().filter_map(|t| t.as_object().cloned()).collect())
                .unwrap_or_default(),
        ),
        None => {
            let id = format!("sp{}", new_uuid().replace('-', ""));
            unnamed.push(("space", id.clone(), project.to_string()));
            (id, Vec::new())
        }
    };
    let ours = stream_tabs(&tabs, stream);
    for (_, tab) in &ours {
        let id = text_of(tab, "id");
        if !id.is_empty() && hc.in_tab(&id)? < MAX_PANES {
            return Ok(Place {
                space,
                tab: id,
                unnamed,
            });
        }
    }
    let number = ours.iter().map(|(n, _)| *n).max().unwrap_or(0) + 1;
    let name = if number == 1 {
        stream.to_string()
    } else {
        format!("{stream}-{number}")
    };
    let tab = format!("tb{}", new_uuid().replace('-', ""));
    unnamed.push(("tab", tab.clone(), name));
    Ok(Place {
        space,
        tab,
        unnamed,
    })
}

/// Name what the launch made, as far as the app has taken it in: what it does not know yet waits
/// for the next look.
pub(super) fn name_place(hc: &dyn Hc, place: &mut Place) {
    place
        .unnamed
        .retain(|(what, id, name)| hc.rename(what, id, name).is_err());
}
