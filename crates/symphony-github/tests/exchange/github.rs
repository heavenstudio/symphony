//! A GitHub repository held in memory, answering the calls the exchange makes as GitHub does:
//! the repository, its issues listed by update and paged, one issue, its creation and edit, and
//! its comments, for a delivery its pull requests, their merge, their auto-merge (which merges
//! by itself once the rulesets let it, as GitHub's does, after each check written) and a
//! commit's checks (listed as GitHub lists them, the newest run of each
//! name by each App, and a little after they are written where a test says so), and for a
//! release its tag and the release of that tag. A test
//! may protect its default branch by rulesets, which it then answers the rules of and holds a
//! merge to, as GitHub does. A test
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
    /// It is carried out and answered: it holds a place, so the fault planted after it meets
    /// the request after.
    Pass,
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
    /// The listings of a head's checks a check written is left out of ([`Fake::list_late`]).
    late: usize,
    /// The checks written and not listed yet, by number: the listings of their head they are
    /// still left out of, and the check listed in their place meanwhile, none for a new one.
    unlisted: BTreeMap<i64, (usize, Option<Value>)>,
    /// The reads of an open pull request that still answer the head it was at before a push to
    /// its branch, and the head it moves to in place of the one pushed ([`Fake::move_late`]).
    lag: (usize, Option<String>),
    /// The pull requests whose head has not moved yet, by number: the reads still answered the
    /// old head, and the head they move to.
    moving: BTreeMap<i64, (usize, String)>,
    /// The GitHub App the request being answered is made as; 0 is a person's token.
    acting: i64,
    /// The number the next pull request or check is given.
    made: i64,
    /// What the repository says of itself beside its number and name.
    about: Value,
    /// The rulesets that apply to the default branch, by number: each with its `rules`, its
    /// `bypass_actors` (shown to a person alone, as GitHub shows them only to who may write the
    /// ruleset, which no App here may), where it is from and when it last changed.
    rulesets: BTreeMap<i64, Value>,
    /// How many times a ruleset changed: each change's `updated_at` is later than the last.
    changes: i64,
    /// Other repositories the token reads, by `OWNER/NAME`: what each says of itself.
    neighbours: BTreeMap<String, Value>,
    /// The requests each App made, as `asked` keeps them for all.
    asked_as: Vec<(i64, String, String)>,
    /// The tags, each with the commit it names and, where it is annotated, its own object's name.
    tags: BTreeMap<String, (String, Option<String>)>,
    /// The tags GitHub made itself, for a release whose tag it did not hold.
    tags_made: Vec<String>,
    releases: Vec<Value>,
    actions: Actions,
}

/// The release verification's workflow as the repository holds it: the runs a dispatch made, and
/// what a test says the next one is and ends as.
pub(crate) struct Actions {
    /// Whether the default branch holds the workflow file.
    pub(crate) workflow: bool,
    /// The commits the repository holds.
    pub(crate) commits: Vec<String>,
    /// The runs, oldest first, as a read of each answers it once it has ended.
    pub(crate) runs: Vec<Value>,
    /// Whether a run is listed among the workflow's.
    pub(crate) listed: bool,
    /// The reads of a run that still answer it in progress.
    pub(crate) pending: usize,
    /// Whether a dispatch's answer names the run it made (`workflow_run_id`), as GitHub's newer
    /// answer does; otherwise it answers 204 with nothing and the run is looked for.
    pub(crate) named: bool,
    /// How the next run ends.
    pub(crate) conclusion: &'static str,
    /// Fields laid over the run a dispatch makes: what a test has GitHub say of it.
    pub(crate) over: Value,
    /// Its jobs, each `(name, conclusion, log)`; a log of none has no download.
    pub(crate) jobs: Vec<(&'static str, &'static str, Option<&'static str>)>,
    /// Its artifacts, each `(name, bytes)`.
    pub(crate) artifacts: Vec<(&'static str, &'static str)>,
    /// What each download's URL serves.
    downloads: BTreeMap<String, String>,
}

/// What signs a download's URL: nothing a test reads back may contain it.
pub(crate) const SIGNATURE: &str = "sig=DOWNLOADsignature0123";

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
            late: 0,
            lag: (0, None),
            moving: BTreeMap::new(),
            unlisted: BTreeMap::new(),
            acting: 0,
            asked_as: Vec::new(),
            tags: BTreeMap::new(),
            tags_made: Vec::new(),
            releases: Vec::new(),
            actions: Actions {
                workflow: true,
                commits: Vec::new(),
                runs: Vec::new(),
                listed: true,
                pending: 0,
                named: false,
                conclusion: "success",
                over: json!({}),
                jobs: Vec::new(),
                artifacts: Vec::new(),
                downloads: BTreeMap::new(),
            },
            made: 100,
            about: json!({
                "default_branch": "main", "archived": false, "allow_squash_merge": true,
                "allow_merge_commit": false, "allow_rebase_merge": false,
                "allow_auto_merge": false, "permissions": {"admin": false, "push": true},
            }),
            rulesets: BTreeMap::new(),
            changes: 0,
            neighbours: BTreeMap::new(),
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

    /// The requests made as the GitHub App `app`: each one's method and path.
    pub(crate) fn asked_as(&self, app: i64) -> Vec<(String, String)> {
        (self.inner().asked_as.iter())
            .filter(|(by, ..)| *by == app)
            .map(|(_, method, path)| (method.clone(), path.clone()))
            .collect()
    }

    /// The annotated tag `name` pushed, naming `commit`.
    pub(crate) fn tag(&self, name: &str, commit: &str) {
        let object = format!("{:0>40}", format!("7a9{}", name.len()));
        (self.inner().tags).insert(name.into(), (commit.into(), Some(object)));
    }

    /// The lightweight tag `name` pushed, naming `commit`.
    pub(crate) fn tag_light(&self, name: &str, commit: &str) {
        (self.inner().tags).insert(name.into(), (commit.into(), None));
    }

    /// The workflow's state, for a test to set and read.
    pub(crate) fn actions<T>(&self, with: impl FnOnce(&mut Actions) -> T) -> T {
        with(&mut self.inner().actions)
    }

    /// The releases, in the order they were made.
    pub(crate) fn releases(&self) -> Vec<Value> {
        self.inner().releases.clone()
    }

    /// The release of `tag` removed there, as a person would.
    pub(crate) fn remove_release(&self, tag: &str) {
        (self.inner().releases).retain(|release| release["tag_name"] != json!(tag));
    }

    /// The tags GitHub made itself for a release.
    pub(crate) fn tags_made(&self) -> Vec<String> {
        self.inner().tags_made.clone()
    }

    /// What GitHub answers a person's `GET path` now, kept out of what was asked.
    pub(crate) fn person_reads(&self, path: &str) -> Response {
        let mut inner = self.inner();
        let acting = std::mem::replace(&mut inner.acting, 0);
        let read = inner.answer("GET", path, None);
        inner.acting = acting;
        read
    }

    /// The numbers of the rulesets there.
    pub(crate) fn ruleset_ids(&self) -> Vec<i64> {
        self.inner().rulesets.keys().copied().collect()
    }

    /// The requests that change the repository.
    pub(crate) fn writes(&self) -> usize {
        self.asked()
            .iter()
            .filter(|(method, ..)| method != "GET")
            .count()
    }

    /// A branch pushed there, as `git push` leaves it: its pull request follows it, at once or
    /// after the reads [`Fake::move_late`] names. A head the branch is at already moves nothing.
    pub(crate) fn push(&self, branch: &str, head: &str) {
        let mut inner = self.inner();
        if inner.branches.insert(branch.into(), head.into()).as_deref() == Some(head) {
            return;
        }
        let (reads, to) = inner.lag.clone();
        let to = to.unwrap_or_else(|| head.to_string());
        let Inner { pulls, moving, .. } = &mut *inner;
        for pull in pulls {
            if pull["head"]["ref"] == json!(branch) && pull["state"] == json!("open") {
                if reads == 0 {
                    pull["head"]["sha"] = json!(to);
                } else {
                    let number = pull["number"].as_i64().unwrap_or_default();
                    moving.insert(number, (reads, to.clone()));
                }
            }
        }
    }

    /// From now on an open pull request answers its next `reads` reads after a push to its
    /// branch at the head it was at before, as GitHub moves its head a little after the push;
    /// then it is at the head pushed, or at `to`, as when someone else pushed after.
    pub(crate) fn move_late(&self, reads: usize, to: Option<&str>) {
        self.inner().lag = (reads, to.map(str::to_string));
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

    /// The default branch protected as a delivery needs it: ruleset 1, which nobody bypasses,
    /// requires a pull request merged by squash, `symphony/eligible` from the App `app`, and
    /// blocks force pushes and deletion.
    pub(crate) fn protect(&self, app: i64) {
        self.ruleset(
            1,
            &json!([]),
            &json!([
                {"type": "pull_request", "parameters": {"allowed_merge_methods": ["squash"]}},
                {"type": "required_status_checks", "parameters": {
                    "strict_required_status_checks_policy": false,
                    "required_status_checks":
                        [{"context": "symphony/eligible", "integration_id": app}],
                }},
                {"type": "non_fast_forward"},
                {"type": "deletion"},
            ]),
        );
    }

    /// Ruleset `id` of the repository, active on the default branch, as it is from now: who
    /// bypasses it (null where no one is shown) and its rules. `Value::Null` rules remove it.
    pub(crate) fn ruleset(&self, id: i64, bypass: &Value, rules: &Value) {
        let mut inner = self.inner();
        if rules.is_null() {
            inner.rulesets.remove(&id);
            return;
        }
        let source = format!("{}/{}", inner.owner, inner.name);
        let changed = inner.changed();
        inner.rulesets.insert(
            id,
            json!({
                "id": id, "name": format!("ruleset {id}"), "target": "branch",
                "source_type": "Repository", "source": source, "enforcement": "active",
                "bypass_actors": bypass, "rules": rules, "updated_at": changed,
            }),
        );
    }

    /// Ruleset `id` changed there, field by field: as on GitHub, any change moves its
    /// `updated_at`, unless `fields` say what it is.
    pub(crate) fn set_ruleset(&self, id: i64, fields: &Value) {
        let mut inner = self.inner();
        let changed = inner.changed();
        let ruleset = inner
            .rulesets
            .get_mut(&id)
            .expect("a ruleset of that number");
        ruleset["updated_at"] = json!(changed);
        for (field, value) in fields.as_object().unwrap() {
            ruleset[field] = value.clone();
        }
    }

    /// Another repository, `full_name`, the token reads as saying `about` of itself from now; a
    /// null `about` makes it unreadable.
    pub(crate) fn neighbour(&self, full_name: &str, about: Value) {
        let mut inner = self.inner();
        if about.is_null() {
            inner.neighbours.remove(full_name);
        } else {
            inner.neighbours.insert(full_name.into(), about);
        }
    }

    /// What the repository says of itself changed, field by field.
    pub(crate) fn set_repository(&self, fields: &Value) {
        let mut inner = self.inner();
        for (field, value) in fields.as_object().unwrap() {
            inner.about[field] = value.clone();
        }
    }

    /// The checks on `head` as GitHub goes by them, the newest run of each name by each App:
    /// each one's name, its conclusion (`in_progress` while it has none) and the App that wrote it.
    pub(crate) fn checks(&self, head: &str) -> Vec<(String, String, i64)> {
        (self.inner().latest(head).iter())
            .map(|run| {
                (
                    run["name"].as_str().unwrap().to_string(),
                    (run["conclusion"].as_str())
                        .or(run["status"].as_str())
                        .unwrap()
                        .to_string(),
                    run["app"]["id"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    /// Every run written on `head`, each one's name and conclusion (`in_progress` while it has
    /// none), oldest first: what a check named again leaves beside it.
    pub(crate) fn runs(&self, head: &str) -> Vec<(String, String)> {
        (self.inner().checks.iter())
            .filter(|run| run["head_sha"] == json!(head))
            .map(|run| {
                (
                    run["name"].as_str().unwrap().to_string(),
                    (run["conclusion"].as_str())
                        .or(run["status"].as_str())
                        .unwrap()
                        .to_string(),
                )
            })
            .collect()
    }

    /// From now on each check written is left out of the next `listings` listings of its head's
    /// checks, the one it rewrote listed in its place meanwhile, as GitHub lists a check a little
    /// after it takes it.
    pub(crate) fn list_late(&self, listings: usize) {
        self.inner().late = listings;
    }

    /// The listings of `head`'s checks asked for.
    pub(crate) fn listings(&self, head: &str) -> usize {
        let inner = self.inner();
        let listing = format!(
            "/repos/{}/{}/commits/{head}/check-runs",
            inner.owner, inner.name
        );
        (inner.asked.iter())
            .filter(|(method, path, _)| method == "GET" && path.starts_with(&listing))
            .count()
    }

    /// A check another App wrote on `head`.
    pub(crate) fn foreign_check(&self, head: &str, name: &str, app: i64) {
        self.foreign_check_as(head, name, app, "success");
    }

    /// A check another App wrote on `head`, concluded `conclusion`.
    pub(crate) fn foreign_check_as(&self, head: &str, name: &str, app: i64, conclusion: &str) {
        let mut inner = self.inner();
        inner.made += 1;
        let id = 700 + inner.made;
        inner.checks.push(json!({
            "id": id, "name": name, "head_sha": head, "status": "completed",
            "conclusion": conclusion, "external_id": "", "app": {"id": app},
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
        inner.asked_as.push((app, method.into(), path.into()));
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
        (
            inner.answer(method, path, body),
            matches!(fault, Some(Fault::LoseAnswer)),
        )
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
        if (method, path) == ("POST", "/graphql") {
            return self.graphql(body);
        }
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let root = format!("/repos/{}/{}", self.owner, self.name);
        let other = (path.strip_prefix("/repos/")).and_then(|name| self.neighbours.get(name));
        if let (Some(about), "GET") = (other, method) {
            return answer(200, about);
        }
        let Some(rest) = path.strip_prefix(&root) else {
            let status = if self.moved { 301 } else { 404 };
            return answer(status, &json!({"message": "Not Found"}));
        };
        let sent: Value = body
            .and_then(|body| serde_json::from_str(body).ok())
            .unwrap_or(Value::Null);
        let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
        match (method, parts.as_slice()) {
            ("GET", []) => {
                let mut about = self.about.clone();
                about["id"] = json!(self.id);
                about["node_id"] = json!(format!("R_{}", self.id));
                about["full_name"] = json!(format!("{}/{}", self.owner, self.name));
                answer(200, &about)
            }
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
            _ => match self.verified(method, &parts, &sent) {
                Some(response) => response,
                None => self.delivery(method, &parts, query, &sent),
            },
        }
    }

    /// The mutations a delivery makes, each naming its pull request by its node: a draft marked
    /// ready, and auto-merge turned on or off.
    fn graphql(&mut self, body: Option<&str>) -> Response {
        let sent: Value = serde_json::from_str(body.unwrap_or_default()).unwrap_or(Value::Null);
        let query = sent["query"].as_str().unwrap_or_default();
        let variables = &sent["variables"];
        let error = |message: &str| {
            answer(
                200,
                &json!({"data": null, "errors": [{"message": message}]}),
            )
        };
        let allowed = self.about["allow_auto_merge"] == json!(true);
        let acting = self.acting;
        let Some(at) = (self.pulls.iter()).position(|pull| pull["node_id"] == variables["id"])
        else {
            return error("Could not resolve to a node with the global id");
        };
        if query.contains("markPullRequestReadyForReview(input: {pullRequestId: $id})") {
            self.pulls[at]["draft"] = json!(false);
            return answer(
                200,
                &json!({"data": {"markPullRequestReadyForReview":
                {"pullRequest": {"isDraft": false}}}}),
            );
        }
        if query.contains("enablePullRequestAutoMerge(input:") {
            let pull = &self.pulls[at];
            let head = pull["head"]["sha"].clone();
            if pull["state"] != json!("open") {
                return error("Pull request is closed");
            }
            if pull["draft"] == json!(true) {
                return error("Pull request is a draft");
            }
            if variables["head"] != head {
                return error("Head sha didn't match expected head");
            }
            if !allowed {
                return error("Auto merge is not allowed for this repository");
            }
            if pull["mergeable_state"] == json!("dirty") {
                return error("Pull request has merge conflicts");
            }
            if pull["mergeable_state"] == json!("clean") && self.broken(&head).is_none() {
                return error("Pull request is in clean status");
            }
            // On already, GitHub answers with no error and keeps the method and text it has.
            if !self.pulls[at]["auto_merge"].is_object() {
                self.pulls[at]["auto_merge"] = json!({
                    "enabled_by": {"login": format!("app-{acting}[bot]")},
                    "merge_method":
                        variables["method"].as_str().unwrap_or_default().to_lowercase(),
                    "commit_title": variables["headline"], "commit_message": variables["body"],
                });
                self.pulls[at]["armed_by"] = json!(acting);
            }
            return answer(
                200,
                &json!({"data": {"enablePullRequestAutoMerge":
                {"pullRequest": {"autoMergeRequest": {"enabledAt": "now"}}}}}),
            );
        }
        if query.contains("disablePullRequestAutoMerge(input: {pullRequestId: $id})") {
            if self.pulls[at]["merged"] == json!(true) {
                return error("Pull request is already merged");
            }
            self.pulls[at]["auto_merge"] = Value::Null;
            return answer(
                200,
                &json!({"data": {"disablePullRequestAutoMerge":
                {"pullRequest": {"autoMergeRequest": null}}}}),
            );
        }
        error("Unknown mutation")
    }

    /// The newest run of each name by each App on `head`, as GitHub lists a commit's checks.
    fn latest(&self, head: &str) -> Vec<Value> {
        let mut newest: Vec<Value> = Vec::new();
        for run in (self.checks.iter()).filter(|run| run["head_sha"] == json!(head)) {
            let same = |other: &Value| other["name"] == run["name"] && other["app"] == run["app"];
            match newest.iter_mut().find(|other| same(other)) {
                Some(other) if other["id"].as_i64() < run["id"].as_i64() => *other = run.clone(),
                Some(_) => {}
                None => newest.push(run.clone()),
            }
        }
        newest
    }

    /// Each open pull request whose auto-merge is on, merged as GitHub merges it: at its head,
    /// once nothing the rulesets require of the App that armed it is missing there.
    fn merge_armed(&mut self) {
        let acting = self.acting;
        for at in 0..self.pulls.len() {
            let pull = &self.pulls[at];
            if pull["state"] != json!("open") || !pull["auto_merge"].is_object() {
                continue;
            }
            if pull["mergeable_state"] != json!("clean") || pull["draft"] == json!(true) {
                continue;
            }
            let head = pull["head"]["sha"].clone();
            self.acting = pull["armed_by"].as_i64().unwrap_or_default();
            let held = self.broken(&head).is_some();
            self.acting = acting;
            if held {
                continue;
            }
            let pull = &mut self.pulls[at];
            pull["merged_as"] = json!({
                "sha": head, "merge_method": pull["auto_merge"]["merge_method"],
                "commit_title": pull["auto_merge"]["commit_title"],
                "commit_message": pull["auto_merge"]["commit_message"],
            });
            pull["state"] = json!("closed");
            pull["merged"] = json!(true);
            pull["merge_commit_sha"] = json!("5".repeat(40));
            pull["merged_by"] = json!({"login": pull["auto_merge"]["enabled_by"]["login"]});
        }
    }

    /// A delivery's calls: a branch's pull requests, their merge and a commit's checks.
    fn delivery(&mut self, method: &str, parts: &[&str], query: &str, sent: &Value) -> Response {
        if let Some(response) = self.released(method, parts, sent) {
            return response;
        }
        if let Some(response) = self.closing(method, parts, sent) {
            return response;
        }
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
                    "node_id": format!("PR_node{number}"), "draft": sent["draft"] == json!(true),
                    "title": sent["title"], "body": sent["body"],
                    "html_url": format!("https://github.com/{}/{}/pull/{number}", self.owner, self.name),
                    "head": {"ref": branch, "sha": head, "repo": repo},
                    "base": {"ref": sent["base"], "repo": repo},
                    "merge_commit_sha": null, "mergeable_state": "clean",
                }));
                answer(201, self.pulls.last().unwrap())
            }
            ("GET", ["pulls", number]) => {
                self.settle(number.parse().unwrap_or_default());
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
                if pull["draft"] == json!(true) {
                    return answer(405, &json!({"message": "Pull Request is still a draft"}));
                }
                if pull["mergeable_state"] != json!("clean") {
                    return answer(405, &json!({"message": "Pull Request is not mergeable"}));
                }
                let head = pull["head"]["sha"].clone();
                if let Some(broken) = self.broken(&head) {
                    return answer(405, &json!({"message": broken}));
                }
                let pull = (self.pulls.iter_mut())
                    .find(|pull| number.parse().ok() == pull["number"].as_i64())
                    .expect("found above");
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
            ("GET", ["commits", head, "check-runs"]) => self.listed(head),
            ("POST", ["check-runs"]) | ("PATCH", ["check-runs", _]) => {
                let written = self.written(parts, sent);
                self.merge_armed();
                written
            }
            ("GET", _) => self.protection(parts, query),
            _ => answer(404, &json!({"message": "Not Found"})),
        }
    }

    /// One read of pull request `number`: its head moved to the one pushed once the reads
    /// [`Fake::move_late`] kept it back for are spent.
    fn settle(&mut self, number: i64) {
        match self.moving.get_mut(&number) {
            Some((left, _)) if *left > 0 => *left -= 1,
            Some(_) => {
                let (_, head) = self.moving.remove(&number).expect("found above");
                for pull in &mut self.pulls {
                    if pull["number"] == json!(number) {
                        pull["head"]["sha"] = json!(head);
                    }
                }
            }
            None => {}
        }
    }

    /// The checks on `head` as GitHub lists them, the newest of each name by each App, a run
    /// planted as listed late in its place by what stood before it.
    fn listed(&mut self, head: &str) -> Response {
        let mut runs = Vec::new();
        for run in &self.latest(head) {
            let id = run["id"].as_i64().unwrap_or_default();
            match self.unlisted.get_mut(&id) {
                Some((left, before)) if *left > 0 => {
                    *left -= 1;
                    runs.extend(before.clone());
                }
                _ => runs.push(run.clone()),
            }
        }
        answer(200, &json!({"total_count": runs.len(), "check_runs": runs}))
    }

    /// A pull request closed unmerged, as `close` asks.
    fn closing(&mut self, method: &str, parts: &[&str], sent: &Value) -> Option<Response> {
        let ("PATCH", ["pulls", number]) = (method, parts) else {
            return None;
        };
        let Some(pull) =
            (self.pulls.iter_mut()).find(|pull| number.parse().ok() == pull["number"].as_i64())
        else {
            return Some(answer(404, &json!({"message": "Not Found"})));
        };
        if pull["merged"] == json!(true) {
            return Some(answer(422, &json!({"message": "Pull request is merged"})));
        }
        if sent["state"] == json!("closed") {
            pull["state"] = json!("closed");
            pull["auto_merge"] = Value::Null;
        }
        Some(answer(200, pull))
    }

    /// A release's calls: one by its tag, its creation, and a tag read as a reference and as its
    /// own object. As GitHub, a release whose tag is not there makes the tag, at the commit asked.
    fn released(&mut self, method: &str, parts: &[&str], sent: &Value) -> Option<Response> {
        let missing = || answer(404, &json!({"message": "Not Found"}));
        Some(match (method, parts) {
            ("GET", ["releases", "tags", tag]) => {
                match (self.releases.iter()).find(|release| release["tag_name"] == json!(tag)) {
                    Some(release) => answer(200, release),
                    None => missing(),
                }
            }
            ("POST", ["releases"]) => {
                let tag = sent["tag_name"].as_str().unwrap_or_default().to_string();
                if (self.releases.iter()).any(|release| release["tag_name"] == json!(tag)) {
                    return Some(answer(
                        422,
                        &json!({"message": "Validation Failed", "errors": [{"code": "already_exists"}]}),
                    ));
                }
                if !self.tags.contains_key(&tag) {
                    let at = sent["target_commitish"].as_str().unwrap_or("main");
                    self.tags.insert(tag.clone(), (at.to_string(), None));
                    self.tags_made.push(tag.clone());
                }
                self.made += 1;
                let mut release = sent.clone();
                release["id"] = json!(600 + self.made);
                release["html_url"] = json!(format!(
                    "https://github.com/{}/{}/releases/tag/{tag}",
                    self.owner, self.name
                ));
                self.releases.push(release.clone());
                answer(201, &release)
            }
            ("GET", ["git", "ref", "tags", tag]) => match self.tags.get(*tag) {
                Some((commit, object)) => answer(
                    200,
                    &json!({"ref": format!("refs/tags/{tag}"), "object": match object {
                        Some(object) => json!({"type": "tag", "sha": object}),
                        None => json!({"type": "commit", "sha": commit}),
                    }}),
                ),
                None => missing(),
            },
            ("GET", ["git", "tags", sha]) => {
                match (self.tags.values()).find(|(_, object)| object.as_deref() == Some(*sha)) {
                    Some((commit, _)) => answer(
                        200,
                        &json!({"sha": sha, "object": {"type": "commit", "sha": commit}}),
                    ),
                    None => missing(),
                }
            }
            _ => return None,
        })
    }

    /// A hosted verification's calls: a commit, the workflow's runs and its dispatch and one
    /// run, then the rest ([`Inner::downloaded`]).
    fn verified(&mut self, method: &str, parts: &[&str], sent: &Value) -> Option<Response> {
        let missing = || answer(404, &json!({"message": "Not Found"}));
        let number = |text: &str| text.parse::<i64>().unwrap_or_default();
        let (id, clock) = (self.id, self.clock);
        let actions = &mut self.actions;
        Some(match (method, parts) {
            ("GET", ["commits", sha]) => {
                if actions.commits.iter().any(|held| held == sha) {
                    answer(200, &json!({"sha": sha}))
                } else {
                    answer(422, &json!({"message": "No commit found for SHA"}))
                }
            }
            (_, ["actions", "workflows", _, ..]) if !actions.workflow => missing(),
            ("GET", ["actions", "workflows", "release-verify.yml", "runs"]) => {
                let mut runs = if actions.listed {
                    actions.runs.clone()
                } else {
                    Vec::new()
                };
                runs.reverse();
                answer(
                    200,
                    &json!({"total_count": runs.len(), "workflow_runs": runs}),
                )
            }
            ("POST", ["actions", "workflows", "release-verify.yml", "dispatches"]) => {
                let run = 9000 + i64::try_from(actions.runs.len()).unwrap();
                let commit = sent["inputs"]["commit"].as_str().unwrap_or_default();
                let mut made = json!({
                    "id": run, "run_attempt": 1, "status": "completed",
                    "conclusion": actions.conclusion, "event": "workflow_dispatch",
                    "path": ".github/workflows/release-verify.yml",
                    "head_branch": sent["ref"], "head_sha": "f".repeat(40),
                    "display_title": format!("Release verification of {commit}"),
                    "repository": {"id": id},
                    "html_url": format!("https://github.com/o/n/actions/runs/{run}"),
                    "run_started_at": stamp(clock), "updated_at": stamp(clock + 2940),
                });
                for (field, value) in actions.over.as_object().into_iter().flatten() {
                    made[field] = value.clone();
                }
                actions.runs.push(made);
                if actions.named {
                    answer(200, &json!({"workflow_run_id": run}))
                } else {
                    answer(204, &Value::Null)
                }
            }
            ("GET", ["actions", "runs", run]) => {
                let Some(found) = (actions.runs.iter()).find(|held| held["id"] == number(run))
                else {
                    return Some(missing());
                };
                let mut found = found.clone();
                if actions.pending > 0 {
                    actions.pending -= 1;
                    found["status"] = json!("in_progress");
                    found["conclusion"] = Value::Null;
                }
                answer(200, &found)
            }
            _ => return self.downloaded(method, parts),
        })
    }

    /// The rest of a hosted verification's calls: a run's jobs and artifacts, and the redirect
    /// each download is answered with.
    fn downloaded(&mut self, method: &str, parts: &[&str]) -> Option<Response> {
        let missing = || answer(404, &json!({"message": "Not Found"}));
        let redirect = |url: String| Response {
            status: 302,
            headers: BTreeMap::from([("location".to_string(), url)]),
            body: String::new(),
        };
        let number = |text: &str| text.parse::<i64>().unwrap_or_default();
        let clock = self.clock;
        let actions = &mut self.actions;
        Some(match (method, parts) {
            ("GET", ["actions", "runs", _, "jobs"]) => {
                let jobs: Vec<Value> = (actions.jobs.iter().enumerate())
                    .map(|(at, (name, conclusion, _))| {
                        json!({
                            "id": 500 + at, "name": name, "conclusion": conclusion,
                            "started_at": stamp(clock), "completed_at": stamp(clock + 90),
                            "html_url": format!("https://github.com/o/n/actions/job/{}", 500 + at),
                        })
                    })
                    .collect();
                answer(200, &json!({"total_count": jobs.len(), "jobs": jobs}))
            }
            ("GET", ["actions", "jobs", job, "logs"]) => {
                let at = usize::try_from(number(job) - 500).unwrap_or(usize::MAX);
                match actions.jobs.get(at).and_then(|(.., log)| *log) {
                    Some(log) => {
                        let url = format!("https://logs.example/job/{job}?{SIGNATURE}");
                        actions.downloads.insert(url.clone(), log.into());
                        redirect(url)
                    }
                    None => missing(),
                }
            }
            ("GET", ["actions", "runs", _, "artifacts"]) => {
                let artifacts: Vec<Value> = (actions.artifacts.iter().enumerate())
                    .map(|(at, (name, _))| json!({"id": 700 + at, "name": name}))
                    .collect();
                answer(
                    200,
                    &json!({"total_count": artifacts.len(), "artifacts": artifacts}),
                )
            }
            ("GET", ["actions", "artifacts", artifact, "zip"]) => {
                let at = usize::try_from(number(artifact) - 700).unwrap_or(usize::MAX);
                match actions.artifacts.get(at) {
                    Some((_, bytes)) => {
                        let url = format!("https://logs.example/artifact/{artifact}?{SIGNATURE}");
                        actions.downloads.insert(url.clone(), (*bytes).into());
                        redirect(url)
                    }
                    None => missing(),
                }
            }
            _ => return None,
        })
    }

    /// The time of a ruleset's next change, each later than the last.
    fn changed(&mut self) -> String {
        self.changes += 1;
        format!(
            "2026-02-01T00:00:{:02}.{:03}Z",
            self.changes / 1000 % 60,
            self.changes % 1000
        )
    }

    /// The rules in effect for a branch, a page of them, and one ruleset.
    fn protection(&self, parts: &[&str], query: &str) -> Response {
        match parts {
            ["rules", "branches", branch @ ..] => {
                let value = |name: &str| -> Option<usize> {
                    (query.split('&'))
                        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))?
                        .parse()
                        .ok()
                };
                let (size, page) = (value("per_page").unwrap_or(30), value("page").unwrap_or(1));
                let rules: Vec<Value> = if branch.join("/") == self.about["default_branch"] {
                    self.rules()
                } else {
                    Vec::new()
                };
                let listed: Vec<&Value> = rules.iter().skip((page - 1) * size).take(size).collect();
                answer(200, &json!(listed))
            }
            ["rulesets", id] => {
                match id.parse().ok().and_then(|id| self.rulesets.get(&id)) {
                    Some(ruleset) => {
                        let mut shown = ruleset.clone();
                        // As GitHub: whether the asker bypasses it, said to anyone.
                        let own = (ruleset["bypass_actors"].as_array().into_iter().flatten()).find(
                            |actor| {
                                actor["actor_type"] == json!("Integration")
                                    && actor["actor_id"] == json!(self.acting)
                            },
                        );
                        shown["current_user_can_bypass"] = json!(match own {
                            None => "never",
                            Some(actor) if actor["bypass_mode"] == json!("pull_request") => {
                                "pull_requests_only"
                            }
                            Some(_) => "always",
                        });
                        // A list the asker may not see is left out, never shown empty: an App's
                        // token administers nothing here, and a person's does.
                        if self.acting != 0 || shown["bypass_actors"].is_null() {
                            shown.as_object_mut().unwrap().remove("bypass_actors");
                        }
                        answer(200, &shown)
                    }
                    None => answer(404, &json!({"message": "Not Found"})),
                }
            }
            _ => answer(404, &json!({"message": "Not Found"})),
        }
    }

    /// The rules in effect for the default branch: every active ruleset's, each naming its own.
    fn rules(&self) -> Vec<Value> {
        (self.rulesets.iter())
            .filter(|(_, ruleset)| ruleset["enforcement"] == json!("active"))
            .flat_map(|(id, ruleset)| {
                let rules = ruleset["rules"].as_array().cloned().unwrap_or_default();
                rules.into_iter().map(move |mut rule| {
                    rule["ruleset_id"] = json!(id);
                    rule["ruleset_source_type"] = ruleset["source_type"].clone();
                    rule["ruleset_source"] = ruleset["source"].clone();
                    rule
                })
            })
            .collect()
    }

    /// The rule a merge of `head` by the one asking breaks, as GitHub holds it: an update
    /// restricted to others, or a required check that does not pass from its App.
    fn broken(&self, head: &Value) -> Option<String> {
        for ruleset in self.rulesets.values() {
            let through = (ruleset["bypass_actors"].as_array().into_iter().flatten()).any(|a| {
                a["actor_type"] == json!("Integration") && a["actor_id"] == json!(self.acting)
            });
            if through || ruleset["enforcement"] != json!("active") {
                continue;
            }
            for rule in ruleset["rules"].as_array().into_iter().flatten() {
                if rule["type"] == json!("update") {
                    return Some(
                        "Repository rule violations found: Cannot update this protected ref."
                            .into(),
                    );
                }
                let required = &rule["parameters"]["required_status_checks"];
                for check in required.as_array().into_iter().flatten() {
                    let latest = self.latest(head.as_str().unwrap_or_default());
                    let passes = latest.iter().any(|run| {
                        run["name"] == check["context"]
                            && run["app"]["id"] == check["integration_id"]
                            && matches!(run["conclusion"].as_str(), Some("success" | "neutral"))
                    });
                    if !passes {
                        return Some(format!(
                            "Repository rule violations found: Required status check {} is expected.",
                            check["context"]
                        ));
                    }
                }
            }
        }
        None
    }

    /// A check written or rewritten, which GitHub takes from an App alone.
    fn written(&mut self, parts: &[&str], sent: &Value) -> Response {
        if self.acting == 0 {
            let said = "You must authenticate via a GitHub App.";
            return answer(403, &json!({"message": said}));
        }
        let mut run = sent.clone();
        run["app"] = json!({"id": self.acting});
        if let ["check-runs", id] = parts {
            let Some(old) = (self.checks.iter_mut()).find(|run| {
                id.parse().ok() == run["id"].as_i64() && run["app"]["id"] == json!(self.acting)
            }) else {
                return answer(404, &json!({"message": "Not Found"}));
            };
            run["id"] = old["id"].clone();
            let before = std::mem::replace(old, run.clone());
            self.unlist(&run, Some(before));
            return answer(200, &run);
        }
        self.made += 1;
        run["id"] = json!(800 + self.made);
        self.checks.push(run.clone());
        self.unlist(&run, None);
        answer(201, &run)
    }

    /// `run`, just written, left out of the next listings [`Fake::list_late`] says, `before`
    /// (what it rewrote) listed in its place.
    fn unlist(&mut self, run: &Value, before: Option<Value>) {
        if self.late > 0 {
            let id = run["id"].as_i64().unwrap_or_default();
            self.unlisted.insert(id, (self.late, before));
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

    fn fetch(&self, url: &str, to: &std::path::Path) -> Result<u16, Failure> {
        match self.inner().actions.downloads.get(url) {
            Some(text) => {
                std::fs::write(to, text).unwrap();
                Ok(200)
            }
            None => Ok(404),
        }
    }
}
