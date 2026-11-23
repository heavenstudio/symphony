//! Who asked for a write (docs/product/analysis/users-and-identity.md, "The actor seam"): every
//! write a Symphony host makes takes the [`Actor`] that asked for it, where the constant `owner`
//! stood. The local host supplies the person at the machine, whose lines read `owner` as they
//! always have; a hosted host supplies the visitor it verified. A plugin is told the public part
//! alone, its [`Sender`], on a write: a board action's submission
//! ([`BoardAction::submit_from`](crate::BoardAction::submit_from)). Reads carry nobody, and no call
//! carries what a person may do.
//!
//! Both types live in the kit because it is the one crate the board, the library and the server
//! all build on. Each is `#[non_exhaustive]` and made by its `new`, so a field a later step adds
//! is a minor of the kit.

use serde::{Deserialize, Serialize};

use crate::execution::Principal;

/// The public part of who sent a write: what a line they write carries and what a plugin is told.
/// No sign-in's subject, email or chat address is in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Sender {
    /// The short name every line they write carries: `owner` for the person at the machine while
    /// a board names nobody.
    pub name: String,
    /// How pages show them.
    pub display: String,
    /// Whose commits are theirs, and whose a write of theirs is authored as. None: the machine's
    /// own git identity, as every save is authored today.
    pub git: Vec<String>,
}

impl Sender {
    pub fn new(name: &str, display: &str, git: Vec<String>) -> Sender {
        Sender {
            name: name.into(),
            display: display.into(),
            git,
        }
    }
}

/// Who asked for a write: the person, as their lines and a plugin name them ([`Sender`]), and the
/// principal a workflow's admission and an approval authority are handed for them, which a host's
/// grants name. What they may do stays the host's grants of that principal.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Actor {
    pub sender: Sender,
    pub principal: Principal,
}

impl Actor {
    pub fn new(sender: Sender, principal: Principal) -> Actor {
        Actor { sender, principal }
    }

    /// The short name the lines they write carry ([`Sender::name`]).
    pub fn name(&self) -> &str {
        &self.sender.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::PrincipalKind;

    #[test]
    fn an_actor_is_named_by_its_senders_short_name() {
        let sender = Sender::new("ana", "Ana Lima", vec!["ana-l".into()]);
        let principal = Principal::new(PrincipalKind::Human, "idp".into(), "u-1".into());
        let actor = Actor::new(sender.clone(), principal.clone());
        assert_eq!(actor.name(), "ana");
        assert_eq!((actor.sender, actor.principal), (sender, principal));
    }

    #[test]
    fn a_sender_reads_back_as_written_and_with_no_git_handles_when_none_are_said() {
        let sender = Sender::new("ana", "Ana Lima", vec!["ana-l".into(), "al".into()]);
        let text = serde_json::to_string(&sender).unwrap();
        assert_eq!(
            text,
            r#"{"name":"ana","display":"Ana Lima","git":["ana-l","al"]}"#
        );
        assert_eq!(serde_json::from_str::<Sender>(&text).unwrap(), sender);
        assert!(serde_json::from_str::<Sender>(r#"{"name":"ana","display":"Ana"}"#).is_err());
    }
}
