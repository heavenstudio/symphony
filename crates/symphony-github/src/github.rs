//! The few calls of GitHub's REST API the issue exchange makes, over a [`Transport`], and what
//! each answer means for a run: an issue listing is every page or said incomplete, a write whose
//! answer was lost is told from one refused, and remote text is cleaned to content before
//! anything local holds it.

use crate::config::Repository;
use crate::transport::{Failure, Request, Response, Transport, rate_limited};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// A page's size, the largest GitHub serves.
const PER_PAGE: usize = 100;
/// The longest title kept of remote text.
const TITLE_CHARS: usize = 200;
/// The longest body kept of remote text, in characters; a longer one is cut.
pub const BODY_CHARS: usize = 60_000;

/// Why a call gave no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// 401: the credential is refused or revoked.
    Unauthorized,
    /// 404 or 410: missing or not accessible, which GitHub does not tell apart.
    NotFound,
    /// 301: the name leads elsewhere now.
    Moved,
    /// The rate limit: the second to wait until, when the answer gave one.
    RateLimited(Option<i64>),
    /// A write whose result is not known: it may have been made.
    Unknown(String),
    /// Nothing was sent.
    NotSent(String),
    /// Any other answer.
    Unexpected(String),
}

impl ApiError {
    /// One line for a person.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            ApiError::Unauthorized => "GitHub refused the credential (401)".into(),
            ApiError::NotFound => "missing or not accessible (404)".into(),
            ApiError::Moved => "moved to another name (301)".into(),
            ApiError::RateLimited(Some(until)) => {
                format!("rate limited until {until} (seconds since 1970)")
            }
            ApiError::RateLimited(None) => "rate limited".into(),
            ApiError::Unknown(why) => format!("result unknown: {why}"),
            ApiError::NotSent(why) | ApiError::Unexpected(why) => why.clone(),
        }
    }
}

/// The repository as GitHub names it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub id: i64,
    pub node_id: String,
    pub full_name: String,
}

/// An issue, its text cleaned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: i64,
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    /// `open` or `closed`.
    pub state: String,
    pub url: String,
    /// The GitHub username of who opened it: a display name, never a board identity.
    pub user: String,
    pub labels: Vec<String>,
    pub updated_at: String,
}

/// A comment on an issue, its text cleaned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub user: String,
    pub body: String,
    pub url: String,
    pub updated_at: String,
}

/// A listing: what was read, in the order of its updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub issues: Vec<Issue>,
    /// No issue was read twice. One read twice moved between pages while they were read, so
    /// another may have moved past unread: such a listing advances no cursor.
    pub stable: bool,
    /// Every page was read, none left past the page limit.
    pub all: bool,
}

/// `text` as one line of content: no control character, runs of whitespace as one space, and
/// no more than a title's length.
#[must_use]
pub fn clean_title(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    words.join(" ").chars().take(TITLE_CHARS).collect()
}

/// `text` as content: line ends as `\n`, no other control character, and no more than a body's
/// length, a longer one cut with a line saying so.
#[must_use]
pub fn clean_body(text: &str) -> String {
    let cleaned: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= BODY_CHARS {
        return cleaned.into();
    }
    let cut: String = cleaned.chars().take(BODY_CHARS).collect();
    format!("{cut}\n\n[cut here: the remote text is longer]")
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_string()
}

fn issue(value: &Value) -> Option<Issue> {
    Some(Issue {
        id: value["id"].as_i64()?,
        node_id: text(value, "node_id"),
        number: value["number"].as_i64()?,
        title: clean_title(&text(value, "title")),
        body: clean_body(&text(value, "body")),
        state: text(value, "state"),
        url: text(value, "html_url"),
        user: clean_title(&text(&value["user"], "login")),
        labels: value["labels"]
            .as_array()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|label| label["name"].as_str().or(label.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        updated_at: text(value, "updated_at"),
    })
}

/// `&since=…` for a cursor that reads as a timestamp, else nothing: no other text is put in
/// a request's path.
fn since_query(since: Option<&str>) -> String {
    since
        .filter(|since| {
            since
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
        })
        .map(|since| format!("&since={since}"))
        .unwrap_or_default()
}

/// The path of `rest` under `repository`'s own: `/repos/OWNER/NAME` and what follows it.
pub(crate) fn path(repository: &Repository, rest: &str) -> String {
    format!("/repos/{}/{}{rest}", repository.owner, repository.name)
}

/// The repository's calls.
pub struct Client<'a> {
    transport: &'a dyn Transport,
    repository: &'a Repository,
    /// Now, in seconds since 1970, which a `Retry-After` counts from.
    now: i64,
}

impl<'a> Client<'a> {
    #[must_use]
    pub fn new(transport: &'a dyn Transport, repository: &'a Repository, now: i64) -> Client<'a> {
        Client {
            transport,
            repository,
            now,
        }
    }

    fn path(&self, rest: &str) -> String {
        path(self.repository, rest)
    }

    /// The JSON of a 2xx answer to `request`.
    fn call(&self, request: &Request) -> Result<(Value, Response), ApiError> {
        let write = request.method != "GET";
        let response = self
            .transport
            .send(request)
            .map_err(|failure| match failure {
                Failure::NotSent(why) => ApiError::NotSent(why),
                Failure::Unknown(why) if write => ApiError::Unknown(why),
                Failure::Unknown(why) => ApiError::Unexpected(why),
            })?;
        if let Some(wait) = rate_limited(&response) {
            return Err(ApiError::RateLimited(wait.until(self.now)));
        }
        match response.status {
            200..=299 => serde_json::from_str(&response.body)
                .map(|value| (value, response))
                .map_err(|error| {
                    let why = format!("GitHub's answer is not JSON: {error}");
                    // A write answered 2xx was made, whatever its body reads as.
                    if write {
                        ApiError::Unknown(why)
                    } else {
                        ApiError::Unexpected(why)
                    }
                }),
            301 => Err(ApiError::Moved),
            401 => Err(ApiError::Unauthorized),
            404 | 410 => Err(ApiError::NotFound),
            500..=599 if write => Err(ApiError::Unknown(format!(
                "GitHub answered {} to {} {}",
                response.status, request.method, request.path
            ))),
            status => Err(ApiError::Unexpected(format!(
                "GitHub answered {status} to {} {}",
                request.method, request.path
            ))),
        }
    }

    fn get(&self, path: String) -> Result<(Value, Response), ApiError> {
        self.call(&Request {
            method: "GET",
            path,
            body: None,
        })
    }

    /// The repository the settings name, as it is now.
    ///
    /// # Errors
    ///
    /// As every call: [`ApiError`].
    pub fn repository(&self) -> Result<Repo, ApiError> {
        let (value, _) = self.get(self.path(""))?;
        let id = value["id"]
            .as_i64()
            .ok_or_else(|| ApiError::Unexpected("the repository has no id".into()))?;
        Ok(Repo {
            id,
            node_id: text(&value, "node_id"),
            full_name: text(&value, "full_name"),
        })
    }

    /// The issues updated at or after `since` (all of them with none), oldest update first,
    /// pull requests left out, reading at most `max_pages` pages.
    ///
    /// # Errors
    ///
    /// As every call. A page that fails fails the listing: no part of it is returned.
    pub fn issues(&self, since: Option<&str>, max_pages: usize) -> Result<Listing, ApiError> {
        let mut issues: Vec<Issue> = Vec::new();
        let mut seen = BTreeSet::new();
        let (mut stable, mut all) = (true, true);
        let mut page = 1;
        loop {
            let path = self.path(&format!(
                "/issues?state=all&sort=updated&direction=asc&per_page={PER_PAGE}&page={page}{}",
                since_query(since)
            ));
            let (value, response) = self.get(path)?;
            let items = value
                .as_array()
                .ok_or_else(|| ApiError::Unexpected("an issue listing is not a list".into()))?;
            for item in items {
                // GitHub lists pull requests among issues.
                if !item["pull_request"].is_null() {
                    continue;
                }
                let read = issue(item)
                    .ok_or_else(|| ApiError::Unexpected("an issue has no id or number".into()))?;
                if seen.insert(read.id) {
                    issues.push(read);
                } else {
                    stable = false;
                }
            }
            let more = response
                .headers
                .get("link")
                .is_some_and(|link| link.contains("rel=\"next\""));
            if !more {
                break;
            }
            if page >= max_pages {
                all = false;
                break;
            }
            page += 1;
        }
        Ok(Listing {
            issues,
            stable,
            all,
        })
    }

    /// Issue `number` as it is now.
    ///
    /// # Errors
    ///
    /// As every call; [`ApiError::NotFound`] is missing or not accessible, never proof it is gone.
    pub fn issue(&self, number: i64) -> Result<Issue, ApiError> {
        let (value, _) = self.get(self.path(&format!("/issues/{number}")))?;
        if !value["pull_request"].is_null() {
            return Err(ApiError::Unexpected(format!("#{number} is a pull request")));
        }
        issue(&value).ok_or_else(|| ApiError::Unexpected("the issue has no id".into()))
    }

    fn write(&self, method: &'static str, path: String, body: &Value) -> Result<Issue, ApiError> {
        let (value, _) = self.call(&Request {
            method,
            path,
            body: Some(body.to_string()),
        })?;
        issue(&value)
            .ok_or_else(|| ApiError::Unknown("the written issue's answer has no id".into()))
    }

    /// A new issue.
    ///
    /// # Errors
    ///
    /// As every call; [`ApiError::Unknown`] when it may have been made.
    pub fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<Issue, ApiError> {
        self.write(
            "POST",
            self.path("/issues"),
            &json!({"title": title, "body": body, "labels": labels}),
        )
    }

    /// Issue `number` with this title and body, its labels, state and people left as they are.
    ///
    /// # Errors
    ///
    /// As [`Client::create`].
    pub fn update(&self, number: i64, title: &str, body: &str) -> Result<Issue, ApiError> {
        self.write(
            "PATCH",
            self.path(&format!("/issues/{number}")),
            &json!({"title": title, "body": body}),
        )
    }

    /// The comments on issue `number` updated at or after `since`, one page of them.
    ///
    /// # Errors
    ///
    /// As every call.
    pub fn comments(&self, number: i64, since: Option<&str>) -> Result<Vec<Comment>, ApiError> {
        let path = self.path(&format!(
            "/issues/{number}/comments?per_page={PER_PAGE}{}",
            since_query(since)
        ));
        let (value, _) = self.get(path)?;
        Ok(value
            .as_array()
            .map(|comments| {
                comments
                    .iter()
                    .filter_map(|comment| {
                        Some(Comment {
                            id: comment["id"].as_i64()?,
                            user: clean_title(&text(&comment["user"], "login")),
                            body: clean_body(&text(comment, "body")),
                            url: text(comment, "html_url"),
                            updated_at: text(comment, "updated_at"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
