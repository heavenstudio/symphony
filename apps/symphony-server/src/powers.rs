//! What a person a board named may write through each route (docs/product/analysis/
//! users-and-identity.md, "The way from one owner", step 3): the powers a write needs of them, read
//! from the board's rows when it is asked, before its body is read ([`crate::write::answer`]). The
//! person at the machine holds every power and is never asked. Whether the thing a decision is on
//! is theirs to decide, of its group (step 4), is the save's to say once the body names it
//! (`symphony::owners::refused_task`, `refused_card`, `refused_settings`).
//!
//! A verdict, its note, a move, a field, Later and a setting are deciding. A line that reaches the
//! board's agents (a comment, a message, an ask and its reply, a quick chat, a quick task) is
//! speaking that directs them, and so is passing a held message on; an attempt's control or view
//! is deciding that directs them. A task's message is speaking alone: the board holds the words of
//! one who may not direct agents for people until someone who may passes them on
//! (`symphony_board::held`). An upload is speaking alone. A change of the board's people is
//! administering; a profile's write needs no power beyond being on the board, as whose profile it
//! may write is `symphony_board::people::change`'s to say. A route this table does not place is
//! the person at the machine's alone, as the agent focus is.

use symphony_board::people::{People, Power, Powers};

/// What a write route needs of a person a board named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
    /// Each of these powers.
    Powers(&'static [Power]),
    /// To be the person at the machine.
    Machine,
}

const DECIDE: Needs = Needs::Powers(&[Power::Decide]);
const SPEAK_TO_AGENTS: Needs = Needs::Powers(&[Power::Speak, Power::DirectAgents]);
const SPEAK: Needs = Needs::Powers(&[Power::Speak]);
const DIRECT_ATTEMPTS: Needs = Needs::Powers(&[Power::Decide, Power::DirectAgents]);
const ADMINISTER: Needs = Needs::Powers(&[Power::Administer]);
/// Being on the board, which every person a board named is.
const ON_THE_BOARD: Needs = Needs::Powers(&[]);

/// What a write to `route` (unscoped: `/api/…`) needs.
pub(crate) fn needs(route: &str) -> Needs {
    match route {
        "/api/postpone" | "/api/decision" | "/api/task-priority" | "/api/task-field"
        | "/api/task-cancel" => DECIDE,
        symphony_api::TASK_ACTION_ROUTE
        | symphony::save::WORKFLOW_APPLY
        | symphony_api::SETTING_WRITE_PATH => DECIDE,
        "/api/comment"
        | "/api/message"
        | "/api/ask"
        | "/api/ask-reply"
        | "/api/quick-chat"
        | "/api/quick-task"
        | symphony_api::TASK_PASS_ROUTE => SPEAK_TO_AGENTS,
        "/api/task-message" | symphony_api::UPLOAD_PATH => SPEAK,
        symphony_api::ATTEMPT_CONTROL_ROUTE | symphony_api::ATTEMPT_VIEW_ROUTE => DIRECT_ATTEMPTS,
        symphony_api::PEOPLE_CHANGE_PATH => ADMINISTER,
        symphony_api::PROFILE_PATH
        | symphony_api::TASK_HAND_ROUTE
        | symphony_api::CARD_HAND_ROUTE => ON_THE_BOARD,
        _ => Needs::Machine,
    }
}

/// Why `name`, holding `held`, may not write where `needs` says: none where they may.
pub(crate) fn refused(name: &str, held: &Powers, needs: Needs) -> Option<String> {
    let Needs::Powers(powers) = needs else {
        return Some("Forbidden: this route answers the person at this machine alone".into());
    };
    (held.lacking(powers))
        .map(|power| format!("Forbidden: {name} may not {} on this board", power.said()))
}

/// Whether `name`, a person of the board `people`, may pass a held message on: they hold what
/// `symphony_api::TASK_PASS_ROUTE` needs. The person at the machine always may, and is no row here.
pub(crate) fn may_pass(people: &People, name: &str) -> bool {
    let needs = needs(symphony_api::TASK_PASS_ROUTE);
    refused(name, &people.powers_of(name), needs).is_none()
}

#[cfg(test)]
#[path = "powers_tests.rs"]
mod tests;
