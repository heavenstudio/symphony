//! The follower: what the app holds of each server from its successive project lists, and what it
//! decides from them: the count of what waits on the owner across every server, the notice a rise
//! or a verdict gives, the line the menu shows for each server and project, and when each server is
//! read next. A server's list carries every project's counts at once, from its memory, so the app
//! reads the list every [`EVERY`] and follows no feed.
//!
//! What waits on the owner in a project is its cards waiting ([`ListedProject::waiting`]) and the
//! questions agents asked them on its tasks ([`TaskAsks::questions`]): the badge's count. A
//! reviewer's verdict asks nothing of them, so it notifies and counts toward nothing. Two servers
//! serving one project are two boards, counted apart; one server reached two ways answers one
//! installation id, and is counted once.

use std::collections::BTreeSet;
use std::time::Duration;

use symphony_api::{ListedProject, ProjectList, TaskAsks};

use crate::address::Server;
use crate::read::Answer;

/// The one person the boards know today, whose asks the app counts.
pub const OWNER: &str = "owner";
/// How often a server that answers is read.
pub const EVERY: Duration = Duration::from_secs(2);
/// How often a server with no project list, an older Symphony, is read again for one.
pub const OLDER_EVERY: Duration = Duration::from_secs(30);
/// The wait before reading again a server that gave no list, doubled with each read in a row that
/// gives none, up to [`LAST_RETRY`]: the feed's own loss rule.
pub const FIRST_RETRY: Duration = Duration::from_secs(1);
pub const LAST_RETRY: Duration = Duration::from_secs(30);

/// What a notice tells of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// More cards wait on the owner.
    Card,
    /// More agents' questions on tasks wait on the owner.
    Question,
    /// A reviewer gave a verdict.
    Verdict,
}

/// One notice for the owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: Kind,
    /// What happened: `3 cards wait on you`.
    pub title: String,
    /// Where, when the app follows more than one: the server's name when it follows more than one
    /// server, the project's when they serve more than one project between them.
    pub place: Option<String>,
    /// More of it: the first card's title, the task's slug.
    pub body: String,
    /// Which server's window opens it, by its place in the followed list.
    pub server: usize,
    /// The page it opens there.
    pub page: String,
}

/// What one read of a server leaves the app holding of it.
#[derive(Clone, Debug, PartialEq)]
enum Reach {
    /// Not read yet.
    Unread,
    /// Its list, at the last read.
    Listed,
    /// An older Symphony, with no list.
    Older,
    /// No list at the last read, and why.
    Lost(String),
}

#[derive(Clone, Debug)]
struct Followed {
    server: Server,
    name: String,
    reach: Reach,
    /// The last list it answered and when, in seconds since the epoch; kept through a loss, so its
    /// last count still shows and a rise past it, once it answers again, still notifies.
    last: Option<(ProjectList, u64)>,
    /// How many reads in a row gave no list.
    losses: u32,
}

/// Every server the app follows, in the order it lists them.
#[derive(Clone, Debug)]
pub struct Follow {
    servers: Vec<Followed>,
}

/// One line of the menu, for a server or one of its projects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What it says.
    pub label: String,
    /// Greyed: a server that does not answer now, whose last count shows.
    pub greyed: bool,
    /// Which server's window it brings in front.
    pub server: usize,
    /// The page that window opens: a project's home; none for a server's own line, which leaves
    /// its window where it is.
    pub page: Option<String>,
}

impl Follow {
    /// The servers `servers`, by address and name, none read yet.
    pub fn new(servers: impl IntoIterator<Item = (Server, String)>) -> Follow {
        Follow {
            servers: servers
                .into_iter()
                .map(|(server, name)| Followed {
                    server,
                    name,
                    reach: Reach::Unread,
                    last: None,
                    losses: 0,
                })
                .collect(),
        }
    }

    /// The servers, in order, with their names.
    pub fn servers(&self) -> impl Iterator<Item = (&Server, &str)> {
        self.servers
            .iter()
            .map(|followed| (&followed.server, followed.name.as_str()))
    }

    /// What reading server `at` at `now` (seconds since the epoch) answered: the notices it gives,
    /// and the wait before it is read again.
    pub fn heard(&mut self, at: usize, answer: Answer, now: u64) -> (Vec<Notice>, Duration) {
        let Some(followed) = self.servers.get_mut(at) else {
            return (Vec::new(), EVERY);
        };
        match answer {
            Answer::Listed(list) => {
                followed.reach = Reach::Listed;
                followed.losses = 0;
                let before = followed.last.replace((list, now)).map(|(list, _)| list);
                let notices = match before {
                    Some(before) if self.counted(at) => self.news(at, &before),
                    _ => Vec::new(),
                };
                (notices, EVERY)
            }
            Answer::Older => {
                followed.reach = Reach::Older;
                followed.losses = 0;
                (Vec::new(), OLDER_EVERY)
            }
            Answer::Lost(why) => {
                followed.reach = Reach::Lost(why);
                followed.losses = followed.losses.saturating_add(1);
                (Vec::new(), retry(followed.losses))
            }
        }
    }

    /// What waits on the owner across every server that answers now, each installation once.
    pub fn total(&self) -> u32 {
        (0..self.servers.len())
            .filter(|&at| self.servers[at].reach == Reach::Listed && self.counted(at))
            .filter_map(|at| self.servers[at].last.as_ref())
            .map(|(list, _)| list.projects.iter().map(waiting).sum::<u32>())
            .sum()
    }

    /// The menu's lines at `now`: each server, then each of its projects with what waits there.
    pub fn lines(&self, now: u64) -> Vec<Line> {
        let mut lines = Vec::new();
        for (at, followed) in self.servers.iter().enumerate() {
            let greyed = matches!(followed.reach, Reach::Lost(_));
            let said = match (&followed.reach, &followed.last) {
                _ if !self.counted(at) => self.first_with(at).map_or_else(String::new, |first| {
                    format!("the same server as {}", self.servers[first].name)
                }),
                (Reach::Unread, _) => String::new(),
                (Reach::Older, _) => "needs a newer Symphony for its count".into(),
                (Reach::Listed, Some((list, _))) => sum(list).to_string(),
                (Reach::Lost(_), Some((list, reached))) => format!(
                    "{}, not answering, last reached {}",
                    sum(list),
                    ago(now.saturating_sub(*reached))
                ),
                (Reach::Lost(_) | Reach::Listed, None) => "not answering".into(),
            };
            lines.push(Line {
                label: joined(&followed.name, &said),
                greyed,
                server: at,
                page: None,
            });
            let shown =
                matches!(followed.reach, Reach::Listed | Reach::Lost(_)) && self.counted(at);
            let Some((list, _)) = followed.last.as_ref().filter(|_| shown) else {
                continue;
            };
            for project in &list.projects {
                let said = match (&project.refused, project.waiting) {
                    (Some(_), _) => "refused by this server".to_owned(),
                    (None, Some(_)) => waiting(project).to_string(),
                    (None, None) => String::new(),
                };
                lines.push(Line {
                    label: format!("    {}", joined(&project.name, &said)),
                    greyed,
                    server: at,
                    page: Some(followed.server.page(&format!("/{}/", project.id))),
                });
            }
        }
        lines
    }

    /// Whether server `at` is counted: no server before it answered its installation id.
    fn counted(&self, at: usize) -> bool {
        self.first_with(at).is_none()
    }

    /// The first server before `at` whose last list answered `at`'s installation id.
    fn first_with(&self, at: usize) -> Option<usize> {
        let id = installation(&self.servers[at])?;
        (0..at).find(|&before| installation(&self.servers[before]) == Some(id))
    }

    /// What server `at`'s newest list says is new since `before`, its list at the read before.
    fn news(&self, at: usize, before: &ProjectList) -> Vec<Notice> {
        let followed = &self.servers[at];
        let Some((now, _)) = &followed.last else {
            return Vec::new();
        };
        let projects: BTreeSet<(usize, &str)> = self
            .servers
            .iter()
            .enumerate()
            .filter_map(|(at, followed)| followed.last.as_ref().map(|(list, _)| (at, list)))
            .flat_map(|(at, list)| list.projects.iter().map(move |p| (at, p.id.as_str())))
            .collect();
        let mut notices = Vec::new();
        for project in &now.projects {
            let Some(was) = before.projects.iter().find(|was| was.id == project.id) else {
                continue;
            };
            let place = [
                (self.servers.len() > 1).then_some(followed.name.as_str()),
                (projects.len() > 1).then_some(project.name.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let place = (!place.is_empty()).then(|| place.join(" · "));
            let notice = |kind, title: String, body: String, page: String| Notice {
                kind,
                title,
                place: place.clone(),
                body,
                server: at,
                page: followed.server.page(&page),
            };
            let home = format!("/{}/", project.id);
            if let (Some(cards), Some(were)) = (project.waiting, was.waiting)
                && cards > were
            {
                let next = project.next.as_ref();
                notices.push(notice(
                    Kind::Card,
                    format!(
                        "{cards} {} on you",
                        plural(cards, "card waits", "cards wait")
                    ),
                    next.map(|card| card.title.clone()).unwrap_or_default(),
                    next.map_or(home.clone(), |card| card.href.clone()),
                ));
            }
            if let (Some(asks), Some(were)) = (&project.asks, &was.asks) {
                let questions = questions(asks);
                if questions > self::questions(were) {
                    notices.push(notice(
                        Kind::Question,
                        format!(
                            "{questions} {} on you",
                            plural(questions, "question waits", "questions wait")
                        ),
                        "An agent asked you on a task".into(),
                        home.clone(),
                    ));
                }
                if let Some(verdict) = asks.verdicts.get(OWNER)
                    && were
                        .verdicts
                        .get(OWNER)
                        .is_none_or(|older| verdict.id > older.id)
                {
                    let title = match verdict.verdict.as_str() {
                        "approve" => format!("{} approved {}", verdict.reviewer, verdict.task),
                        "changes" => {
                            format!("{} asks for changes on {}", verdict.reviewer, verdict.task)
                        }
                        other => format!("{}: {other} on {}", verdict.reviewer, verdict.task),
                    };
                    notices.push(notice(
                        Kind::Verdict,
                        title,
                        "A reviewer's verdict".into(),
                        format!("/{}/task/{}", project.id, verdict.task),
                    ));
                }
            }
        }
        notices
    }
}

/// The wait before the next read after `losses` reads in a row gave no list: 1 s, doubled each
/// time, up to 30 s.
pub fn retry(losses: u32) -> Duration {
    let doubled = FIRST_RETRY.saturating_mul(1u32 << losses.saturating_sub(1).min(16));
    doubled.min(LAST_RETRY)
}

/// How long ago `seconds` was, in words.
pub fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "less than a minute ago".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!(
            "{} {} ago",
            seconds / 86_400,
            plural(seconds / 86_400, "day", "days")
        ),
    }
}

/// What waits on the owner in `project`: its cards and its tasks' questions.
fn waiting(project: &ListedProject) -> u32 {
    project
        .waiting
        .unwrap_or(0)
        .saturating_add(project.asks.as_ref().map_or(0, questions))
}

fn questions(asks: &TaskAsks) -> u32 {
    asks.questions.get(OWNER).copied().unwrap_or(0)
}

fn sum(list: &ProjectList) -> u32 {
    list.projects.iter().map(waiting).sum()
}

fn installation(followed: &Followed) -> Option<&str> {
    followed.last.as_ref()?.0.installation.as_deref()
}

fn joined(name: &str, said: &str) -> String {
    if said.is_empty() {
        name.to_owned()
    } else {
        format!("{name} — {said}")
    }
}

fn plural<'a>(count: impl Into<u64>, one: &'a str, more: &'a str) -> &'a str {
    if count.into() == 1 { one } else { more }
}

#[cfg(test)]
#[path = "follow_tests.rs"]
mod tests;
