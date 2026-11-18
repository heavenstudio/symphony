//! `[board.external_postgresql]`: where a hosted board's PostgreSQL is, said as references to the
//! deployment's secrets and as the connection budget the deployment enforces. Every build reads
//! and judges the table, so a project file is checked the same anywhere; only a build with the
//! `hosted-postgresql` feature connects ([`super::hosted`]).
//!
//! The table holds no credential: `url_env` names the variable that holds the PostgreSQL URI, or
//! `url_file_env` the variable that holds the path of a mounted file of it, exactly one of them;
//! `ca_file_env` the variable naming the certificate authority's file, and `client_cert_file_env`
//! with `client_key_file_env` a client certificate where the endpoint asks for one.
//!
//! The budget is the deployment's whole use of the login role, counted for every process that can
//! be alive at once: each class's `processes` (old and new revisions during a rollout, draining
//! instances, each process of an instance) times its `pool_max`, the `maintenance` connections of
//! a migration or an export, the role's `other_users`, and `headroom`, together at most
//! `role_connection_cap`, the cap the platform holds the role to. A process names its class
//! (`class_env`), and every connection it makes takes one of its class's slots in the database
//! ([`super::hosted`]), so the classes' totals are held by the database and not by each process's
//! own count.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The table's name in the project file, as a refusal names a key of it.
pub const TABLE: &str = "[board.external_postgresql]";
/// The class a migration or an export runs as: its slots are the budget's `maintenance`.
pub const MAINTENANCE: &str = "maintenance";
/// The most seconds a wait may be set to: a day.
const MOST_SECONDS: u32 = 86_400;
/// The most connections a budget may count: more than any role's cap.
const MOST_CONNECTIONS: u32 = 10_000;

/// `[board.external_postgresql]`, as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The variable holding the PostgreSQL URI.
    pub url_env: Option<String>,
    /// The variable holding the path of a file that holds the URI.
    pub url_file_env: Option<String>,
    /// The variable holding the path of the certificate authority's PEM file.
    pub ca_file_env: String,
    /// The variables holding the paths of a client certificate and its key, both or neither.
    pub client_cert_file_env: Option<String>,
    pub client_key_file_env: Option<String>,
    /// The variable holding this process's class, one of the budget's.
    pub class_env: String,
    /// How long a connection may take to be made.
    #[serde(default = "ten")]
    pub connect_timeout_seconds: u32,
    /// How long a request may wait for a connection of the pool.
    #[serde(default = "ten")]
    pub acquire_timeout_seconds: u32,
    /// How long a request may take once it has its connection.
    #[serde(default = "three_hundred")]
    pub request_timeout_seconds: u32,
    /// How many requests may wait for a connection at once; one more is refused at once.
    #[serde(default = "sixteen")]
    pub queue_max: u32,
    /// The deployment's connection budget.
    pub budget: Option<Budget>,
}

fn ten() -> u32 {
    10
}

fn three_hundred() -> u32 {
    300
}

fn sixteen() -> u32 {
    16
}

/// `[board.external_postgresql.budget]`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// The most connections the platform lets the login role hold.
    pub role_connection_cap: u32,
    /// Connections of the role this deployment's Symphony processes do not make.
    #[serde(default)]
    pub other_users: u32,
    /// Connections kept for a migration or an export, which never run at once past this.
    pub maintenance: u32,
    /// Connections left free.
    pub headroom: u32,
    /// Each class of process, by name.
    pub classes: BTreeMap<String, Class>,
}

/// One class of process: how many can be alive at once, and each one's pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub processes: u32,
    pub pool_max: u32,
}

/// What a process of one class may hold: its own pool, and its class's slots in the database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    pub class: String,
    /// The most connections this process holds at once.
    pub pool_max: u32,
    /// The most connections every process of the class holds together.
    pub class_max: u32,
}

/// The table the project file at the main checkout `root` holds: none when it holds none, or
/// there is no file. Refused, naming the file, when the file does not parse or the table is not
/// one [`Settings::validate`] takes. Only that table is read; the rest of the file is the
/// Symphony's.
pub fn of(root: &std::path::Path) -> Result<Option<Settings>, String> {
    let path = root.join(crate::project::FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let named = |why: String| format!("{}: {why}", path.display());
    let table: toml::Table = text.parse().map_err(|error| named(format!("{error}")))?;
    let Some(written) = table
        .get("board")
        .and_then(|board| board.get("external_postgresql"))
    else {
        return Ok(None);
    };
    judged(written.clone()).map(Some).map_err(named)
}

/// The table as `written` in a project file, read and judged ([`Settings::validate`]): what
/// the board's own reader and a project's check of its file both take.
pub fn judged(written: toml::Value) -> Result<Settings, String> {
    let settings: Settings = written
        .try_into()
        .map_err(|error| format!("{TABLE}: {error}"))?;
    settings.validate()?;
    Ok(settings)
}

impl Settings {
    /// The table judged on its shape alone: its references named, its waits and queue in range
    /// and its budget within the role's cap. A refusal names the key, never a value an
    /// environment holds.
    pub fn validate(&self) -> Result<(), String> {
        match (&self.url_env, &self.url_file_env) {
            (Some(_), Some(_)) => {
                return Err(format!(
                    "{TABLE} names both url_env and url_file_env; it names exactly one"
                ));
            }
            (None, None) => {
                return Err(format!(
                    "{TABLE} names neither url_env nor url_file_env; it names exactly one"
                ));
            }
            _ => {}
        }
        if self.client_cert_file_env.is_some() != self.client_key_file_env.is_some() {
            return Err(format!(
                "{TABLE} names one of client_cert_file_env and client_key_file_env; a client \
                 certificate needs both"
            ));
        }
        let mut seen = Vec::new();
        for (key, name) in [
            ("url_env", self.url_env.as_deref()),
            ("url_file_env", self.url_file_env.as_deref()),
            ("ca_file_env", Some(self.ca_file_env.as_str())),
            ("client_cert_file_env", self.client_cert_file_env.as_deref()),
            ("client_key_file_env", self.client_key_file_env.as_deref()),
            ("class_env", Some(self.class_env.as_str())),
        ] {
            let Some(name) = name else { continue };
            if !variable(name) {
                return Err(format!(
                    "{TABLE} {key} is not the name of an environment variable (letters, digits \
                     and _, not starting with a digit)"
                ));
            }
            if seen.contains(&name) {
                return Err(format!(
                    "{TABLE} {key} names a variable another key names; each names its own"
                ));
            }
            seen.push(name);
        }
        for (key, seconds) in [
            ("connect_timeout_seconds", self.connect_timeout_seconds),
            ("acquire_timeout_seconds", self.acquire_timeout_seconds),
            ("request_timeout_seconds", self.request_timeout_seconds),
        ] {
            if !(1..=MOST_SECONDS).contains(&seconds) {
                return Err(format!(
                    "{TABLE} {key} is {seconds}; it is from 1 to {MOST_SECONDS} seconds"
                ));
            }
        }
        if self.queue_max > MOST_CONNECTIONS {
            return Err(format!(
                "{TABLE} queue_max is {}; it is at most {MOST_CONNECTIONS}",
                self.queue_max
            ));
        }
        let Some(budget) = &self.budget else {
            return Err(format!(
                "{TABLE} has no budget; a hosted board needs the deployment's connection budget \
                 ({TABLE_BUDGET})"
            ));
        };
        budget.validate()
    }

    /// What a process of `class` may hold, by the budget; refused for a class the budget does not
    /// name. The table is judged first ([`Settings::validate`]).
    pub fn share(&self, class: &str) -> Result<Share, String> {
        self.validate()?;
        let budget = self.budget.as_ref().expect("validated above");
        if class == MAINTENANCE {
            return Ok(Share {
                class: class.into(),
                pool_max: 1,
                class_max: budget.maintenance,
            });
        }
        let Some(found) = budget.classes.get(class) else {
            return Err(format!(
                "{TABLE_BUDGET} has no class {class:?}, which this process says it is; its \
                 classes are {} and {MAINTENANCE}",
                budget
                    .classes
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        Ok(Share {
            class: class.into(),
            pool_max: found.pool_max,
            class_max: found.processes * found.pool_max,
        })
    }
}

const TABLE_BUDGET: &str = "[board.external_postgresql.budget]";

impl Budget {
    /// Every connection the budget counts: each class's processes times its pool, the role's
    /// other users, maintenance and headroom.
    pub fn counted(&self) -> u64 {
        let classes: u64 = (self.classes.values())
            .map(|class| u64::from(class.processes) * u64::from(class.pool_max))
            .sum();
        classes
            + u64::from(self.other_users)
            + u64::from(self.maintenance)
            + u64::from(self.headroom)
    }

    fn validate(&self) -> Result<(), String> {
        if self.classes.is_empty() {
            return Err(format!(
                "{TABLE_BUDGET} names no class; every process that connects is of one"
            ));
        }
        if self.maintenance == 0 {
            return Err(format!(
                "{TABLE_BUDGET} maintenance is 0; a migration needs one connection"
            ));
        }
        for (name, class) in &self.classes {
            if !class_name(name) || name == MAINTENANCE {
                return Err(format!(
                    "{TABLE_BUDGET} class {name:?} is not a class's name (lower-case letters, \
                     digits and - or _, at most 32, and not {MAINTENANCE})"
                ));
            }
            if class.processes == 0 || class.pool_max == 0 {
                return Err(format!(
                    "{TABLE_BUDGET} class {name:?} has 0 processes or a pool of 0; each is at \
                     least 1"
                ));
            }
            if class.processes > MOST_CONNECTIONS || class.pool_max > MOST_CONNECTIONS {
                return Err(format!(
                    "{TABLE_BUDGET} class {name:?} counts more than {MOST_CONNECTIONS}"
                ));
            }
        }
        for (key, count) in [
            ("role_connection_cap", self.role_connection_cap),
            ("other_users", self.other_users),
            ("maintenance", self.maintenance),
            ("headroom", self.headroom),
        ] {
            if count > MOST_CONNECTIONS {
                return Err(format!(
                    "{TABLE_BUDGET} {key} is {count}; it is at most {MOST_CONNECTIONS}"
                ));
            }
        }
        let counted = self.counted();
        if counted > u64::from(self.role_connection_cap) {
            return Err(format!(
                "{TABLE_BUDGET} counts {counted} connections (each class's processes times its \
                 pool_max, other_users, maintenance and headroom) and role_connection_cap is {}: \
                 the deployment could hold more than the role may",
                self.role_connection_cap
            ));
        }
        Ok(())
    }
}

/// Whether `name` can name an environment variable.
fn variable(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && name.len() <= 128
}

/// Whether `name` can name a class: what a slot's key is made from.
fn class_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_files_table_is_read_judged_and_absent_without_one() {
        let root = std::env::temp_dir().join(format!("board-external-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(of(&root), Ok(None), "no file");
        let file = root.join(crate::project::FILE);
        std::fs::write(&file, "[board]\nguide = \"TASKS.md\"\n").unwrap();
        assert_eq!(of(&root), Ok(None), "no table");
        let written = "[board.external_postgresql]\nurl_env = \"URL\"\nca_file_env = \"CA\"\n\
            class_env = \"CLASS\"\n[board.external_postgresql.budget]\nrole_connection_cap = 5\n\
            maintenance = 1\nheadroom = 1\n[board.external_postgresql.budget.classes]\n\
            web = { processes = 1, pool_max = 2 }\n";
        std::fs::write(&file, written).unwrap();
        let read = of(&root).unwrap().unwrap();
        assert_eq!(read.share("web").unwrap().class_max, 2);
        // Over its cap, a key it does not have, and text that is no TOML: each names the file.
        for (broken, why) in [
            (written.replace("cap = 5", "cap = 3"), "role_connection_cap"),
            (
                written.replace("url_env", "url"),
                "[board.external_postgresql]",
            ),
            ("[board".to_string(), "symphony.toml"),
        ] {
            std::fs::write(&file, broken).unwrap();
            let refused = of(&root).unwrap_err();
            assert!(
                refused.contains(why) && refused.starts_with(&file.display().to_string()),
                "{refused}"
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    const WHOLE: &str = r#"
url_env = "SYMPHONY_DATABASE_URL"
ca_file_env = "SYMPHONY_DATABASE_CA_FILE"
class_env = "SYMPHONY_DATABASE_CLASS"

[budget]
role_connection_cap = 12
maintenance = 1
headroom = 2

[budget.classes]
web = { processes = 2, pool_max = 3 }
worker = { processes = 1, pool_max = 3 }
"#;

    fn read(text: &str) -> Settings {
        toml::from_str(text).unwrap()
    }

    /// `WHOLE` with `from` replaced by `to`, refused for what it then says.
    fn refused(from: &str, to: &str) -> String {
        assert!(WHOLE.contains(from), "{from}");
        read(&WHOLE.replace(from, to)).validate().unwrap_err()
    }

    #[test]
    fn a_whole_table_is_taken_with_its_default_waits_and_its_classes_shares() {
        let settings = read(WHOLE);
        settings.validate().unwrap();
        assert_eq!(
            (
                settings.connect_timeout_seconds,
                settings.acquire_timeout_seconds,
                settings.request_timeout_seconds,
                settings.queue_max
            ),
            (10, 10, 300, 16)
        );
        assert_eq!(settings.budget.as_ref().unwrap().counted(), 12);
        assert_eq!(
            settings.share("web").unwrap(),
            Share {
                class: "web".into(),
                pool_max: 3,
                class_max: 6
            }
        );
        assert_eq!(
            settings.share(MAINTENANCE).unwrap(),
            Share {
                class: MAINTENANCE.into(),
                pool_max: 1,
                class_max: 1
            }
        );
        let unknown = settings.share("cron").unwrap_err();
        assert!(
            unknown.contains("no class \"cron\"") && unknown.contains("web, worker"),
            "{unknown}"
        );
    }

    #[test]
    fn exactly_one_url_reference_and_a_whole_client_certificate_are_asked_for() {
        let both = refused("url_env", "url_file_env = \"F\"\nurl_env");
        assert!(both.contains("both url_env and url_file_env"), "{both}");
        let neither = refused("url_env = \"SYMPHONY_DATABASE_URL\"", "");
        assert!(neither.contains("neither url_env nor"), "{neither}");
        let file = read(&WHOLE.replace("url_env", "url_file_env"));
        file.validate().unwrap();
        let half = refused("class_env", "client_cert_file_env = \"C\"\nclass_env");
        assert!(half.contains("needs both"), "{half}");
        read(&WHOLE.replace(
            "class_env",
            "client_cert_file_env = \"C\"\nclient_key_file_env = \"K\"\nclass_env",
        ))
        .validate()
        .unwrap();
    }

    #[test]
    fn a_reference_is_a_variables_name_of_its_own_and_never_a_value() {
        for value in ["postgresql://u:p@h/d", "", "1ST", "A-B", &"A".repeat(129)] {
            let why = refused("SYMPHONY_DATABASE_URL", value);
            assert!(
                why.contains("url_env is not the name of an environment variable"),
                "{why}"
            );
            assert!(value.is_empty() || !why.contains(value), "{why}");
        }
        let shared = refused("SYMPHONY_DATABASE_CA_FILE", "SYMPHONY_DATABASE_URL");
        assert!(
            shared.contains("ca_file_env names a variable another"),
            "{shared}"
        );
    }

    #[test]
    fn waits_and_the_queue_are_held_in_range() {
        for (key, value) in [
            ("connect_timeout_seconds", 0),
            ("acquire_timeout_seconds", 86_401),
            ("request_timeout_seconds", 0),
        ] {
            let why = refused("class_env", &format!("{key} = {value}\nclass_env"));
            assert!(
                why.contains(key) && why.contains("from 1 to 86400"),
                "{why}"
            );
        }
        let why = refused("class_env", "queue_max = 10001\nclass_env");
        assert!(why.contains("queue_max is 10001"), "{why}");
        read(&WHOLE.replace("class_env", "queue_max = 0\nclass_env"))
            .validate()
            .unwrap();
    }

    #[test]
    fn a_missing_or_overdrawn_budget_is_refused_naming_what_it_counts() {
        let none = read(&WHOLE[..WHOLE.find("[budget]").unwrap()])
            .validate()
            .unwrap_err();
        assert!(none.contains("has no budget"), "{none}");
        let over = refused("processes = 1", "processes = 2");
        assert!(
            over.contains("counts 15 connections") && over.contains("role_connection_cap is 12"),
            "{over}"
        );
        let other = refused("maintenance = 1", "maintenance = 1\nother_users = 1");
        assert!(other.contains("counts 13 connections"), "{other}");
        let idle = refused("maintenance = 1", "maintenance = 0");
        assert!(idle.contains("maintenance is 0"), "{idle}");
        let large = refused("headroom = 2", "headroom = 10001");
        assert!(large.contains("headroom is 10001"), "{large}");
        let wide = refused("pool_max = 3 }\nworker", "pool_max = 10001 }\nworker");
        assert!(wide.contains("counts more than 10000"), "{wide}");
        let empty = refused("processes = 2", "processes = 0");
        assert!(empty.contains("0 processes or a pool of 0"), "{empty}");
    }

    #[test]
    fn a_class_is_named_plainly_and_maintenance_is_the_budgets_own() {
        for name in ["Web", "a b", MAINTENANCE, &"w".repeat(33)] {
            let why = refused("web =", &format!("\"{name}\" ="));
            assert!(why.contains("is not a class's name"), "{why}");
        }
        let cut = WHOLE.find("web =").unwrap();
        let none = read(&WHOLE[..cut]).validate().unwrap_err();
        assert!(none.contains("names no class"), "{none}");
        // A budget that leaves the classes out altogether is no budget the reader takes.
        let without = &WHOLE[..WHOLE.find("[budget.classes]").unwrap()];
        assert!(toml::from_str::<Settings>(without).is_err());
    }

    #[test]
    fn an_unknown_key_is_refused_by_the_reader() {
        for text in [
            WHOLE.replace("class_env", "password = \"x\"\nclass_env"),
            WHOLE.replace("headroom", "slack = 1\nheadroom"),
            WHOLE.replace("pool_max = 3 }\nworker", "pool_max = 3, min = 1 }\nworker"),
        ] {
            assert!(toml::from_str::<Settings>(&text).is_err(), "{text}");
        }
    }
}
