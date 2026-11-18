//! A GitHub repository held in memory, answering the calls the exchange makes as GitHub does:
//! the repository, its issues listed by update and paged, one issue, its creation and edit, and
//! its comments, and for a delivery its pull requests, their merge and a commit's checks. A test
//! sets it up, plants a fault on a coming request, and reads what it was
//! asked. It answers as a [`Transport`] directly, and `http.rs` serves it to a real `curl`.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use symphony_github::transport::{Failure, Request, Response, Transport};

/// What goes wrong with one request.
#[derive(Clone, Debug)]
pub(crate) enum Fault {
    /// It is carried out, and its answer never arrives.
    LoseAnswer,
    /// It is not carried out: this status and these headers are the answer.
    Status(u16, Vec<(&'static str, String)>),
}

struct Inner {
    id: i64,
    owner: String,
    name: String,
    /// The name a request must use; another is answered 301 (`moved`) or 404.
    moved: bool,
    issues: Vec<Value>,
    comments: BTreeMap<i64, Vec<Value>>,
    next: i64,
    clock: i64,
    page_size: usize,
    /// Faults by the method and the start of the path they wait for, taken in order.
    faults: Vec<(String, String, Fault)>,
    /// Issues that are updated, so moved to the end, as page `0` of a listing is served.
    shift_after_first_page: Vec<i64>,
    asked: Vec<(String, String, Option<String>)>,
    /// The branches pushed, each at its head.
    branches: BTreeMap<String, String>,
    pulls: Vec<Value>,
    checks: Vec<Value>,
    /// The GitHub App the request being answered is made as; 0 is a person's token.
    acting: i64,
    /// The number the next pull request or check is given.
    made: i64,
}

pub(crate) struct Fake(Mutex<Inner>);

fn stamp(clock: i64) -> String {
    format!(
        "2026-01-01T{:02}:{:02}:{:02}Z",
        clock / 3600,
        clock / 60 % 60,
        clock % 60
    )
}

impl Fake {
    pub(crate) fn new(id: i64, owner: &str, name: &str) -> Fake {
        Fake(Mutex::new(Inner {
            id,
            owner: owner.into(),
            name: name.into(),
            moved: false,
            issues: Vec::new(),
            comments: BTreeMap::new(),
            next: 1,
            clock: 0,
            page_size: 100,
            faults: Vec::new(),
            shift_after_first_page: Vec::new(),
            asked: Vec::new(),
            branches: BTreeMap::new(),
            pulls: Vec::new(),
            checks: Vec::new(),
            acting: 0,
            made: 100,
        }))
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new issue as a GitHub user opened it: its number.
    pub(crate) fn open(&self, title: &str, body: &str, labels: &[&str]) -> i64 {
        self.inner().create(title, body, labels, "octocat")
    }

    /// The issue edited there, as a GitHub user would.
    pub(crate) fn edit(&self, number: i64, title: &str, body: &str) {
        self.inner().update(number, Some(title), Some(body), None);
    }

    pub(crate) fn close(&self, number: i64) {
        self.inner().update(number, None, None, Some("closed"));
    }

    pub(crate) fn comment(&self, number: i64, user: &str, body: &str) -> i64 {
        let mut inner = self.inner();
        inner.clock += 1;
        let id = 9000 + inner.clock;
        let at = stamp(inner.clock);
        inner.comments.entry(number).or_default().push(json!({
            "id": id, "user": {"login": user}, "body": body, "updated_at": at,
            "html_url": format!("https://github.com/o/n/issues/{number}#issuecomment-{id}"),
        }));
        inner.update(number, None, None, None);
        id
    }

    /// The issue is no longer reachable: 404 from now on.
    pub(crate) fn hide(&self, number: i64) {
        self.inner()
            .issues
            .retain(|issue| issue["number"] != json!(number));
    }

    /// Another repository answers at this name now.
    pub(crate) fn replace_repository(&self, id: i64) {
        self.inner().id = id;
    }

    /// The repository was renamed: its old name answers 301.
    pub(crate) fn rename(&self, name: &str) {
        let mut inner = self.inner();
        inner.name = name.into();
        inner.moved = true;
    }

    pub(crate) fn page_size(&self, size: usize) {
        self.inner().page_size = size;
    }

    pub(crate) fn shift_after_first_page(&self, numbers: &[i64]) {
        self.inner().shift_after_first_page = numbers.to_vec();
    }

    /// The next request of `method` whose path starts with `path` meets `fault`.
    pub(crate) fn fault(&self, method: &str, path: &str, fault: Fault) {
        self.inner()
            .faults
            .push((method.into(), path.into(), fault));
    }

    pub(crate) fn issue(&self, number: i64) -> Value {
        self.inner()
            .issues
            .iter()
            .find(|issue| issue["number"] == json!(number))
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub(crate) fn issues(&self) -> Vec<Value> {
        self.inner().issues.clone()
    }

    /// Every request made so far: its method, path and body.
    pub(crate) fn asked(&self) -> Vec<(String, String, Option<String>)> {
        self.inner().asked.clone()
    }

    /// The requests that change the repository.
    pub(crate) fn writes(&self) -> usize {
        self.asked()
            .iter()
            .filter(|(method, ..)| method != "GET")
            .count()
    }

    /// A branch pushed there, as `git push` leaves it: its pull request follows it.
    pub(crate) fn push(&self, branch: &str, head: &str) {
        let mut inner = self.inner();
        inner.branches.insert(branch.into(), head.into());
        for pull in &mut inner.pulls {
            if pull["head"]["ref"] == json!(branch) && pull["state"] == json!("open") {
                pull["head"]["sha"] = json!(head);
            }
        }
    }

    pub(crate) fn pulls(&self) -> Vec<Value> {
        self.inner().pulls.clone()
    }

    /// Pull request `number` changed there, field by field.
    pub(crate) fn set_pull(&self, number: i64, fields: &Value) {
        let mut inner = self.inner();
        let pull = (inner.pulls.iter_mut())
            .find(|pull| pull["number"] == json!(number))
            .expect("a pull request of that number");
        for (field, value) in fields.as_object().unwrap() {
            pull[field] = value.clone();
        }
    }

    /// The checks on `head`: each one's name, conclusion and the App that wrote it.
    pub(crate) fn checks(&self, head: &str) -> Vec<(String, String, i64)> {
        (self.inner().checks.iter())
            .filter(|run| run["head_sha"] == json!(head))
            .map(|run| {
                (
                    run["name"].as_str().unwrap().to_string(),
                    run["conclusion"].as_str().unwrap().to_string(),
                    run["app"]["id"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    /// A check another App wrote on `head`.
    pub(crate) fn foreign_check(&self, head: &str, name: &str, app: i64) {
        let mut inner = self.inner();
        inner.made += 1;
        let id = 700 + inner.made;
        inner.checks.push(json!({
            "id": id, "name": name, "head_sha": head, "status": "completed",
            "conclusion": "success", "external_id": "", "app": {"id": app},
        }));
    }

    /// The answer to a request, and whether it is to be lost on the way back.
    pub(crate) fn handle(&self, method: &str, path: &str, body: Option<&str>) -> (Response, bool) {
        self.handle_as(0, method, path, body)
    }

    /// The same, the request made as the GitHub App `app`.
    pub(crate) fn handle_as(
        &self,
        app: i64,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> (Response, bool) {
        let mut inner = self.inner();
        inner.acting = app;
        inner
            .asked
            .push((method.into(), path.into(), body.map(str::to_string)));
        let planted = inner
            .faults
            .iter()
            .position(|(m, p, _)| m == method && path.starts_with(p.as_str()));
        let fault = planted.map(|at| inner.faults.remove(at).2);
        if let Some(Fault::Status(status, headers)) = &fault {
            return (
                Response {
                    status: *status,
                    headers: headers
                        .iter()
                        .map(|(name, value)| ((*name).to_string(), value.clone()))
                        .collect(),
                    body: "{\"message\":\"planted\"}".into(),
                },
                false,
            );
        }
        (inner.answer(method, path, body), fault.is_some())
    }
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: body.to_string(),
    }
}

impl Inner {
    fn create(&mut self, title: &str, body: &str, labels: &[&str], user: &str) -> i64 {
        self.clock += 1;
        let number = self.next;
        self.next += 1;
        self.issues.push(json!({
            "id": 5000 + number, "node_id": format!("I_node{number}"), "number": number,
            "title": title, "body": body, "state": "open",
            "html_url": format!("https://github.com/{}/{}/issues/{number}", self.owner, self.name),
            "user": {"login": user},
            "labels": labels.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
            "updated_at": stamp(self.clock),
        }));
        number
    }

    fn update(
        &mut self,
        number: i64,
        title: Option<&str>,
        body: Option<&str>,
        state: Option<&str>,
    ) {
        self.clock += 1;
        let at = stamp(self.clock);
        if let Some(issue) = self
            .issues
            .iter_mut()
            .find(|i| i["number"] == json!(number))
        {
            for (field, value) in [("title", title), ("body", body), ("state", state)] {
                if let Some(value) = value {
                    issue[field] = json!(value);
                }
            }
            issue["updated_at"] = json!(at);
        }
    }

    fn answer(&mut self, method: &str, path: &str, body: Option<&str>) -> Response {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let root = format!("/repos/{}/{}", self.owner, self.name);
        let Some(rest) = path.strip_prefix(&root) else {
            let status = if self.moved { 301 } else { 404 };
            return answer(status, &json!({"message": "Not Found"}));
        };
        let sent: Value = body
            .and_then(|body| serde_json::from_str(body).ok())
            .unwrap_or(Value::Null);
        let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
        match (method, parts.as_slice()) {
            ("GET", []) => answer(
                200,
                &json!({"id": self.id, "node_id": format!("R_{}", self.id),
                        "full_name": format!("{}/{}", self.owner, self.name)}),
            ),
            ("GET", ["issues"]) => self.list(query),
            ("POST", ["issues"]) => {
                let labels: Vec<&str> = sent["labels"]
                    .as_array()
                    .map(|labels| labels.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let number = self.create(
                    sent["title"].as_str().unwrap_or_default(),
                    sent["body"].as_str().unwrap_or_default(),
                    &labels,
                    "symphony-bot",
                );
                answer(201, &self.find(&number.to_string()).unwrap_or(Value::Null))
            }
            ("GET", ["issues", number]) => match self.find(number) {
                Some(issue) => answer(200, &issue),
                None => answer(404, &json!({"message": "Not Found"})),
            },
            ("PATCH", ["issues", number]) => {
                let Some(found) = self.find(number) else {
                    return answer(404, &json!({"message": "Not Found"}));
                };
                let number = found["number"].as_i64().unwrap_or_default();
                self.update(
                    number,
                    sent["title"].as_str(),
                    sent["body"].as_str(),
                    sent["state"].as_str(),
                );
                answer(200, &self.find(&number.to_string()).unwrap_or(Value::Null))
            }
            ("GET", ["issues", number, "comments"]) => {
                let since = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("since="))
                    .unwrap_or("");
                let number: i64 = number.parse().unwrap_or_default();
                let comments: Vec<Value> = self
                    .comments
                    .get(&number)
                    .map(|comments| {
                        comments
                            .iter()
                            .filter(|c| c["updated_at"].as_str().unwrap_or_default() >= since)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                answer(200, &json!(comments))
            }
            _ => self.delivery(method, &parts, query, &sent),
        }
    }

    /// A delivery's calls: a branch's pull requests, their merge and a commit's checks.
    fn delivery(&mut self, method: &str, parts: &[&str], query: &str, sent: &Value) -> Response {
        match (method, parts) {
            ("GET", ["pulls"]) => {
                let value = |name: &str| {
                    (query.split('&')).find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
                };
                let head = value("head")
                    .and_then(|head| head.split_once(':'))
                    .map(|(_, r)| r);
                let listed: Vec<Value> = (self.pulls.iter())
                    .filter(|pull| {
                        pull["state"] == json!("open")
                            && Some(pull["head"]["ref"].as_str().unwrap_or_default()) == head
                            && pull["base"]["ref"].as_str() == value("base")
                    })
                    .cloned()
                    .collect();
                answer(200, &json!(listed))
            }
            ("POST", ["pulls"]) => {
                let branch = sent["head"].as_str().unwrap_or_default();
                let Some(head) = self.branches.get(branch).cloned() else {
                    return answer(422, &json!({"message": "Validation Failed: no such head"}));
                };
                let number = 100 + i64::try_from(self.pulls.len()).unwrap();
                let repo = json!({"id": self.id});
                self.pulls.push(json!({
                    "number": number, "state": "open", "merged": false,
                    "title": sent["title"], "body": sent["body"],
                    "html_url": format!("https://github.com/{}/{}/pull/{number}", self.owner, self.name),
                    "head": {"ref": branch, "sha": head, "repo": repo},
                    "base": {"ref": sent["base"], "repo": repo},
                    "merge_commit_sha": null, "mergeable_state": "clean",
                }));
                answer(201, self.pulls.last().unwrap())
            }
            ("GET", ["pulls", number]) => {
                match self
                    .pulls
                    .iter()
                    .find(|pull| number.parse().ok() == pull["number"].as_i64())
                {
                    Some(pull) => answer(200, pull),
                    None => answer(404, &json!({"message": "Not Found"})),
                }
            }
            ("PUT", ["pulls", number, "merge"]) => {
                let Some(pull) = (self.pulls.iter_mut())
                    .find(|pull| number.parse().ok() == pull["number"].as_i64())
                else {
                    return answer(404, &json!({"message": "Not Found"}));
                };
                if pull["head"]["sha"] != sent["sha"] {
                    return answer(409, &json!({"message": "Head branch was modified"}));
                }
                if pull["mergeable_state"] != json!("clean") {
                    return answer(405, &json!({"message": "Pull Request is not mergeable"}));
                }
                let squash = "5".repeat(40);
                pull["state"] = json!("closed");
                pull["merged"] = json!(true);
                pull["merge_commit_sha"] = json!(squash);
                pull["merged_as"] = sent.clone();
                answer(
                    200,
                    &json!({"merged": true, "sha": squash, "message": "merged"}),
                )
            }
            ("GET", ["commits", head, "check-runs"]) => {
                let runs: Vec<Value> = (self.checks.iter())
                    .filter(|run| run["head_sha"] == json!(head))
                    .cloned()
                    .collect();
                answer(200, &json!({"total_count": runs.len(), "check_runs": runs}))
            }
            ("POST", ["check-runs"]) | ("PATCH", ["check-runs", _]) => {
                if self.acting == 0 {
                    let said = "You must authenticate via a GitHub App.";
                    return answer(403, &json!({"message": said}));
                }
                let mut run = sent.clone();
                run["app"] = json!({"id": self.acting});
                if let ["check-runs", id] = parts {
                    let Some(old) = (self.checks.iter_mut()).find(|run| {
                        id.parse().ok() == run["id"].as_i64()
                            && run["app"]["id"] == json!(self.acting)
                    }) else {
                        return answer(404, &json!({"message": "Not Found"}));
                    };
                    run["id"] = old["id"].clone();
                    *old = run.clone();
                    return answer(200, &run);
                }
                self.made += 1;
                run["id"] = json!(800 + self.made);
                self.checks.push(run.clone());
                answer(201, &run)
            }
            _ => answer(404, &json!({"message": "Not Found"})),
        }
    }

    fn find(&self, number: &str) -> Option<Value> {
        let number: i64 = number.parse().ok()?;
        self.issues
            .iter()
            .find(|issue| issue["number"] == json!(number))
            .cloned()
    }

    fn list(&mut self, query: &str) -> Response {
        let value = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        };
        let since = value("since").unwrap_or("").to_string();
        let page: usize = value("page")
            .and_then(|page| page.parse().ok())
            .unwrap_or(1);
        let mut listed: Vec<Value> = self
            .issues
            .iter()
            .filter(|issue| issue["updated_at"].as_str().unwrap_or_default() >= since.as_str())
            .cloned()
            .collect();
        listed.sort_by_key(|issue| issue["updated_at"].as_str().unwrap_or_default().to_string());
        let start = (page - 1) * self.page_size;
        let items: Vec<Value> = listed
            .iter()
            .skip(start)
            .take(self.page_size)
            .cloned()
            .collect();
        let mut response = answer(200, &json!(items));
        if start + self.page_size < listed.len() {
            response.headers.insert(
                "link".into(),
                format!(
                    "<https://api.github.com/next?page={}>; rel=\"next\"",
                    page + 1
                ),
            );
        }
        if page == 1 {
            for number in std::mem::take(&mut self.shift_after_first_page) {
                self.update(number, None, None, None);
            }
        }
        response
    }
}

impl Transport for Fake {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        let (response, lost) = self.handle(request.method, &request.path, request.body.as_deref());
        if lost {
            Err(Failure::Unknown(
                "the connection ended with no answer".into(),
            ))
        } else {
            Ok(response)
        }
    }
}
