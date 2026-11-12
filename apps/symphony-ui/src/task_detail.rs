//! Task detail and agent views, compiled separately from the summary-only board.
use super::*;
use std::rc::Rc;
use symphony_api::{Agents, TaskPage};

#[cfg(target_arch = "wasm32")]
#[component]
pub(crate) fn LiveTaskPage(
    route: Memo<String>,
    data: Signal<crate::task_live::Data>,
    connected: bool,
) -> Element {
    use crate::task_live;
    let agents = data.read().agents.clone().map(|mut agents| {
        agents.machine_work = crate::machine_work::connection(agents.machine_work, connected);
        agents
    });
    // A link to a part of the agents page (`/agents#machine-context`) names an element that
    // mounts only once the agents arrive: locate it then, once per route.
    let mut located = use_signal(String::new);
    use_effect(move || {
        let path = route();
        if !path.starts_with("/task/") && data.read().agents.is_some() && *located.peek() != path {
            located.set(path);
            crate::navigation::scroll_anchor();
        }
    });
    // The page's error is drawn in its notice stack (`live::BoardPage`), out of the page's flow;
    // a page with nothing yet to show says in its body what is missing or failed.
    rsx! {
        if let Some(slug) = route().strip_prefix("/task/") {
            if let Some(page) = data.read().detail.clone().filter(|p| p.task.slug == slug) {
                TaskDetail { key: "{page.task.slug}", work: crate::machine_work::connection(page.machine_work.clone(), connected), tasks: page.children.clone(), page, entries: data.read().entries.clone(), older: data.read().before.is_some(), loaded: data.read().thread_loaded, offer: data.read().approval.clone(), onhistory: move |older| { spawn(task_live::history(older,route,data)); }, onchildren: move |more| { spawn(task_live::children(more,route,data)); }, onoffer: move |_| { spawn(task_live::offer(route,data)); } }
            } else if data.read().missing.as_deref() == Some(slug) {
                crate::trouble::Trouble { title: "No task {slug}", text: "The board has no task by this name: it may have been removed, or the link names it wrongly.", back: Some(("/tasks".to_string(), "All tasks".to_string())), retry: move |_| { spawn(task_live::refresh(route, data)); } }
            } else if !data.read().error.is_empty() {
                crate::trouble::Trouble { title: "The task could not be loaded", text: crate::trouble::failed(&data.read().error), retry: move |_| { spawn(task_live::refresh(route, data)); } }
            } else { p { "Loading task…" } }
        } else if let Some(agents) = agents {
            AgentsView { agents, tasks: data.read().tasks.clone().unwrap_or_default(), total: data.read().agent_total, more: data.read().has_more_agents(false), lost_more: data.read().has_more_agents(true), onmore: move |lost| { spawn(task_live::more_agents(lost,route,data)); } }
        } else if !data.read().error.is_empty() {
            crate::trouble::Trouble { title: "The agents could not be loaded", text: crate::trouble::failed(&data.read().error), retry: move |_| { spawn(task_live::load(route, data)); } }
        } else { p { "Loading agents…" } }
    }
}

#[component(lazy)]
pub fn TaskDetail(
    page: TaskPage,
    #[props(default)] work: symphony_api::MachineWork,
    tasks: Vec<Task>,
    entries: Vec<symphony_api::TaskEntry>,
    older: bool,
    loaded: bool,
    onhistory: EventHandler<bool>,
    onchildren: EventHandler<bool>,
    onoffer: EventHandler<()>,
    offer: Option<(symphony_api::TaskOffer, f64)>,
) -> Element {
    let task = &page.task;
    let approval = offer
        .filter(|(_, revision)| *revision == task.updated)
        .map(|(offer, _)| offer);
    rsx! { article { class: "task", "data-slug": "{task.slug}",
        div { class: "task-head card",
        p { class: "eyebrow", code { "{task.slug}" } }
        h1 { "{task.title}" }
        div { class: "task-state row", span { class: "state pill st-{task.status}", "{task.status.replace('_', \" \")}" } span { class: "pill", "p{task.priority}" } "by {task.created_by}"
            OwnerControls { slug: task.slug.clone(), priority: task.priority, status: task.status.clone() }
        }
        ul { class: "facts",
            if let Some(parent) = &task.parent { li { "part of " a { href: crate::project::href(&format!("/task/{parent}")), "{parent}" } } }
            for dependency in &task.depends { li { "after " a { href: crate::project::href(&format!("/task/{dependency}")), "{dependency}" } } }
            for link in &task.links { li { "implements " a { href: crate::project::href(&format!("/{link}")), "{link}" } } }
        }
        Roles { roles: task.roles.clone() }
        {phases_view(&work, &Scope::Task(&task.slug), true)}
        }
        if page.children_count > 0 { Near { id: "subtasks-{task.slug}", height: 60, onenter: move |_| onchildren.call(false),
            h2 { "Subtasks " span { class: "count", "{page.children_count}" } }
            ul { for child in tasks.iter() { Near { key: "{child.slug}", list_item: true, id: "child-{child.slug}", height: 32, a { href: crate::project::href(&format!("/task/{}", child.slug)), "{child.title}" } " · {child.status.replace('_', \" \")}" } } }
            if tasks.is_empty() { crate::components::button::Button { onclick: move |_| onchildren.call(false), "Load subtasks" } }
            else if tasks.len() < page.children_count { crate::components::button::Button { onclick: move |_| onchildren.call(true), "Load more subtasks" } }
        } }
        Brief { slug: task.slug.clone(), revision: page.brief_revision.clone(), blocks: page.blocks.clone(), omitted: page.brief_omitted, next: page.brief_next }
        h2 { "Thread" }
        Near { id: "thread-region-{task.slug}", height: 200, onenter: move |_| onhistory.call(false),
            div { id: "task-thread",
                if older { crate::components::button::Button { onclick: move |_| onhistory.call(true), "Load earlier messages" } }
                if !loaded { crate::components::button::Button { onclick: move |_| onhistory.call(false), "Load messages" } }
                // One timeline: every message is an item of the one list its line runs down.
                ul { class: "history", for entry in entries { Near { key: "{entry.message.id}", list_item: true, id: "message-{entry.message.id}", height: 100, TaskMessage { entry } } } }
            }
        }
        Near { id: "composer-{task.slug}", height: 200,
            OfferLoader { key: "offer-{task.slug}-{task.updated}", onoffer }
            Composer { slug: task.slug.clone(), roles: task.roles.clone(), approval }
        }
    } }
}

#[component]
fn OwnerControls(slug: String, priority: i64, status: String) -> Element {
    let current_priority = use_memo(use_reactive!(|priority| priority));
    let mut selected = use_signal(|| priority.to_string());
    let mut confirming = use_signal(|| false);
    let mut cancelled = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut result = use_signal(String::new);
    use_effect(move || selected.set(current_priority().to_string()));
    let terminal = cancelled() || matches!(status.as_str(), "done" | "cancelled");
    use_effect(use_reactive!(|terminal| {
        if terminal {
            confirming.set(false);
        }
    }));
    let priority_slug = slug.clone();
    rsx! {
        div { class: "task-controls row",
                label { class: "inline-control", r#for: "task-priority", "Priority "
                    crate::components::select::SelectField { id: "task-priority", label: "Priority", value: selected(), disabled: busy(), on_change: move |value: String| {
                        if busy() { return; }
                        selected.set(value.clone());
                        busy.set(true);
                        result.set(String::new());
                        let slug = priority_slug.clone();
                        spawn(async move {
                            #[cfg(target_arch = "wasm32")]
                            match crate::task_live::post("/api/task-priority", &[("slug", &slug), ("priority", &value)]).await {
                                Ok(_) => result.set("Priority saved".into()),
                                Err(err) => { selected.set(current_priority().to_string()); result.set(err); }
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            { let _ = (slug, value); }
                            busy.set(false);
                        });
                    },
                        options: (0..=4).map(|value| (value.to_string(), format!("p{value}"))).collect::<Vec<_>>(),
                    }
                }
                if !terminal && !confirming() {
                    crate::components::button::Button { r#type: "button", disabled: busy(), onclick: move |_| confirming.set(true), "Cancel task" }
                }
            if !terminal && confirming() {
                div { class: "row", role: "group", aria_label: "Cancel task confirmation",
                    span { "Cancel this task?" }
                    crate::components::button::Button { r#type: "button", disabled: busy(), onclick: move |_| {
                        if busy() { return; }
                        busy.set(true);
                        result.set(String::new());
                        let slug = slug.clone();
                        spawn(async move {
                            #[cfg(target_arch = "wasm32")]
                            match crate::task_live::post("/api/task-cancel", &[("slug", &slug)]).await {
                                Ok(_) => { cancelled.set(true); confirming.set(false); result.set("Task cancelled".into()); }
                                Err(err) => result.set(err),
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            { let _ = (slug, &mut cancelled); }
                            busy.set(false);
                        });
                    }, "Confirm cancellation" }
                    crate::components::button::Button { r#type: "button", disabled: busy(), onclick: move |_| confirming.set(false), "Keep task" }
                }
            }
            if !result().is_empty() { p { role: "status", "{result}" } }
        }
    }
}

#[component]
fn TaskMessage(entry: symphony_api::TaskEntry) -> Element {
    let message = &entry.message;
    let who = if message.sender == "owner" {
        "owner"
    } else if message.kind == "system" {
        "system"
    } else {
        "agent"
    };
    rsx! { div { class: "entry {who}",
        time { "{entry.stamp}" } " " span { class: "from", "{message.sender}" if let Some(to) = &message.recipient { " → {to}" } }
        if message.kind == "verdict" { b { "verdict " } }
        // An owner message is literal text, unless it names an upload: the server then sends
        // its blocks, the text kept as typed and the files drawn.
        if who == "agent" || who == "owner" && !entry.blocks.is_empty() {
            div { class: "entry-text", crate::block_view::BlocksView { blocks: entry.blocks } }
        } else {
            div { class: "entry-text literal", "{message.text}" }
        }
    } }
}

#[component]
fn OfferLoader(onoffer: EventHandler<()>) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_hook(|| onoffer.call(()));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = onoffer;
    rsx! {}
}

/// This component's signals survive every task/agent event and reconnect. A submitted draft is
/// cleared only on success, and only if the owner has not edited it while the request ran.
#[component]
fn Composer(slug: String, roles: Vec<Role>, approval: Option<symphony_api::TaskOffer>) -> Element {
    let draft_key = format!("task-draft:{slug}");
    let input_key = draft_key.clone();
    let mut draft = use_signal(|| {
        crate::platform::current()
            .load(&draft_key)
            .unwrap_or_default()
    });
    use_effect(move || crate::platform::current().save(&draft_key, &draft()));
    let to_key = format!("task-to:{slug}");
    let mut recipient = use_signal(|| crate::platform::current().load(&to_key).unwrap_or_default());
    let mut busy = use_signal(|| false);
    let mut result = use_signal(String::new);
    let mut attach = crate::attach::use_attach(format!("task-draft:{slug}"));
    let send_slug = slug.clone();
    let mut send = move |_| {
        if busy() || attach.busy() || draft().trim().is_empty() && attach.is_empty() {
            return;
        }
        busy.set(true);
        let typed = draft();
        let text = attach.compose(&typed);
        let to = recipient();
        let slug = send_slug.clone();
        spawn(async move {
            #[cfg(target_arch = "wasm32")]
            match crate::task_live::post(
                "/api/task-message",
                &[("slug", &slug), ("text", &text), ("to", &to)],
            )
            .await
            {
                Ok(_) => {
                    // The files went with it; no new one can arrive while it is sent.
                    attach.clear();
                    if draft() == typed {
                        crate::platform::current().save(&format!("task-draft:{slug}"), "");
                        draft.set(String::new());
                        result.set("Sent".into());
                    } else {
                        result.set(String::new());
                    }
                }
                Err(err) => result.set(err),
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = (slug, typed, text, to, &mut draft, &mut result, &mut attach);
            }
            busy.set(false);
        });
    };
    let mut keyboard_send = send.clone();
    rsx! {
        form { id: "task-send", class: "verdict", "data-posting": busy(), "data-attach": attach.zone(), onsubmit: move |event| { event.prevent_default(); send(()); },
            label { r#for: "task-message", "Message" }
            crate::components::textarea::Textarea { id: "task-message", value: "{draft}", placeholder: "Message (⌘/Ctrl+Enter sends). Agents on this task hear it wherever they run.",
                oninput: move |event: FormEvent| { let text = event.value(); crate::platform::current().save(&input_key,&text); draft.set(text); if result() == "Sent" { result.set(String::new()); } },
                onkeydown: move |event: KeyboardEvent| { if event.key() == Key::Enter && (event.modifiers().ctrl() || event.modifiers().meta()) { event.prevent_default(); keyboard_send(()); } }
            }
            crate::attach::AttachControls { attach, disabled: busy() }
            div { class: "row", label { class: "inline-control", "to " crate::components::select::SelectField {
                label: "Message recipient", value: recipient(),
                on_change: move |value: String| { crate::platform::current().save(&to_key,&value); recipient.set(value); },
                options: std::iter::once((String::new(), "everyone on it".to_string()))
                    .chain(["@executor", "@reviewer", "@orchestrator"].map(|role| (role.to_string(), role.to_string())))
                    .chain(roles.into_iter().map(|role| (role.handle.clone(), role.handle))).collect::<Vec<_>>(),
            } } crate::components::button::Button { r#type: "submit", disabled: busy() || attach.busy(), "Send" }
            if let Some(symphony_api::TaskOffer { action, label, branch, payload: Some(payload) }) = approval {
                code { class: "approval-branch", "{branch}" }
                crate::components::button::Button { r#type: "button", disabled: busy(), onclick: move |_| {
                busy.set(true); let slug = slug.clone(); let action = action.clone(); let payload = payload.clone();
                spawn(async move {
                    #[cfg(target_arch = "wasm32")]
                    result.set(match crate::task_live::post(symphony_api::TASK_ACTION_ROUTE, &[("slug", &slug), ("action", &action), ("payload", &payload)]).await {
                        Ok(done) => js_sys::Reflect::get(&done, &"said".into()).ok().and_then(|said| said.as_string()).unwrap_or_default(),
                        Err(err) => err,
                    });
                    #[cfg(not(target_arch = "wasm32"))]
                    { let _ = (slug, action, payload, &mut result); }
                    busy.set(false);
                });
            }, "✓ {label}" } }
            }
            p { role: "status", "{result}" }
        }
    }
}

#[component(lazy)]
pub fn AgentsView(
    mut agents: Agents,
    tasks: Vec<Task>,
    total: usize,
    more: bool,
    lost_more: bool,
    onmore: EventHandler<bool>,
) -> Element {
    agents.agents.sort_by(|a, b| a.handle.cmp(&b.handle));
    let tasks = Rc::new(tasks);
    let work: SharedWork = std::mem::take(&mut agents.machine_work).into();
    rsx! {
        div { class: "page-head", div { h1 { "Agents" } p { class: "lead", "Who is connected, and what they are working on." } } }
        p { class: "muted", "Terminal panes: {agents.probes.panes} · Test slots: {agents.probes.slots}" }
        h2 { "Happening now" }
        div { class: "agents-now", for task in tasks.iter().filter(|t| t.has_status(symphony_api::TaskStatus::InProgress) || t.has_status(symphony_api::TaskStatus::Review)) { Near { key: "{task.slug}", id: "now-{task.slug}", height: 120, TaskRow { task: task.clone(), work: work.clone() } } } }
        {crate::machine_work::machines_view(&work, true)}
        h2 { "Connected" }
        div { class: "agents agents-card card", for agent in agents.agents.iter().filter(|a| a.health != "lost" && a.health != "left") {
            Near { key: "{agent.handle}", id: "agent-{agent.handle}", height: 240, AgentRow { agent: agent.clone(), tasks: tasks.clone(), work: work.clone() } }
        } }
        if more && agents.agents.iter().filter(|a| a.health != "lost" && a.health != "left").count() < total { Near { id: "more-agents".to_string(), onenter: move |_| onmore.call(false), crate::components::button::Button { onclick: move |_| onmore.call(false), "Load more agents" } } }
        details { summary { onclick: move |_| onmore.call(true), "Lost agents" } div { for agent in agents.agents.into_iter().filter(|a| a.health == "lost") { Near { key: "{agent.handle}", id: "agent-{agent.handle}", height: 120, AgentRow { agent, tasks: tasks.clone(), work: work.clone() } } } if lost_more { crate::components::button::Button { onclick: move |_| onmore.call(true), "Load more lost agents" } } } }
    }
}

#[component]
fn AgentRow(agent: symphony_api::Agent, tasks: Rc<Vec<Task>>, work: SharedWork) -> Element {
    let mut following = use_signal(|| false);
    let following_rows = use_signal(Vec::<Task>::new);
    let following_total = use_signal(|| 0_usize);
    let following_cursor = use_signal(|| None::<String>);
    let following_trouble = use_signal(String::new);
    let handle = agent.handle.clone();
    let more_handle = handle.clone();
    let following_count = agent
        .tasks
        .iter()
        .filter(|t| t.status != "done" && t.status != "cancelled" && t.role == "orchestrator")
        .count();
    let mut followed: Vec<_> = following_rows()
        .into_iter()
        .map(|row| {
            tasks
                .iter()
                .find(|t| t.slug == row.slug)
                .cloned()
                .unwrap_or(row)
        })
        .filter(|t| t.status != "done" && t.status != "cancelled")
        .collect();
    for task in tasks.iter().filter(|t| {
        t.status != "done"
            && t.status != "cancelled"
            && t.roles
                .iter()
                .any(|r| r.handle == agent.handle && r.role == "orchestrator")
    }) {
        if !followed.iter().any(|t| t.slug == task.slug) {
            followed.push(task.clone());
        }
    }
    followed.sort_by(|a, b| (a.priority, &a.slug).cmp(&(b.priority, &b.slug)));
    let held: Vec<_> = tasks
        .iter()
        .filter_map(|task| {
            task.roles
                .iter()
                .find(|r| r.handle == agent.handle)
                .map(|role| (task, role))
        })
        .collect();
    let mut finished: Vec<_> = held
        .iter()
        .filter(|(t, _)| t.status == "done" || t.status == "cancelled")
        .collect();
    finished.sort_by(|(a, _), (b, _)| b.updated.total_cmp(&a.updated));
    rsx! { div { class: "agent",
        strong { "{agent.handle}" } " " span { class: "chip health-{agent.health}", "{agent.health}" }
        p { class: "muted", "{agent.kind} · {agent.model} · {agent.state} · {agent.branch} · {agent.worktree.rsplit('/').next().unwrap_or(&agent.worktree)}" }
        {phases_view(&work, &Scope::Agent(&agent.handle), true)}
        details { class: "agent-connection", summary { "Connection details" }
        if let Some(address) = &agent.callback {
            p { "Native callback: " code { "{address}" } }
            p { "Send: " code { "just agent-send {agent.handle} \"text\"" } }
        } else {
            p { "Send on a task: " code { "just tasks say SLUG \"text\" --to {agent.handle}" } }
            p { "No channel registered — " code { "just tasks inbox --as {agent.handle}" } }
            p { "Register in that agent's session: " code { "just tasks callback --as {agent.handle}" } }
        }
        }
        if agent.chat { p { "Chat goes to {agent.handle}." } }
        if let Some(parent) = agent.parent { p { "parent: {parent}" } }
        if let Some(slots) = agent.slots { p { "open tasks: {slots.open} of {slots.cap}" } }
        if let Some(run) = agent.state_detail { p { "{run.recipe} · {run.phase}" } }
        ul { for (task, role) in held.iter().filter(|(t, r)| t.status != "done" && t.status != "cancelled" && r.role != "orchestrator") {
            li { a { href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" } " · {role.role} · {task.status.replace('_', \" \")}" }
        } }
        // An agent following nothing offers nothing to open.
        if following_count > 0 { details { summary { onclick: move |_| { if !following() { load_following(handle.clone(),false,following_rows,following_total,following_cursor,following_trouble); } following.set(!following()); }, "{following_label(following_count)}" }
            if following() { p { role: "status", "{following_trouble}" } ul { for task in &followed {
                Near { key: "{task.slug}", list_item: true, id: "following-{agent.handle}-{task.slug}", height: 40, a { href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" } " · {task.status.replace('_', \" \")}" }
            } } if followed.len() < following_count { crate::components::button::Button { onclick: move |_| load_following(more_handle.clone(),true,following_rows,following_total,following_cursor,following_trouble), "Load more followed tasks" } } }
        } }
        if !finished.is_empty() { p { class: "muted", "Finished: " for (task, _) in finished.iter().take(5) { a { href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" } " · " } if finished.len() > 5 { "and {finished.len() - 5} more" } } }
        if let Some(said) = agent.last_message { p { a { href: crate::project::href(&format!("/task/{}", said.slug)), "Latest message" } ": {said.text}" } }
    } }
}

/// The disclosure of the open tasks an agent orchestrates, counted in words.
fn following_label(count: usize) -> String {
    let noun = crate::words::plural(u32::try_from(count).unwrap_or(u32::MAX), "task");
    format!("Following {count} open {noun}")
}

fn load_following(
    handle: String,
    more: bool,
    mut rows: Signal<Vec<Task>>,
    mut total: Signal<usize>,
    mut cursor: Signal<Option<String>>,
    mut trouble: Signal<String>,
) {
    #[cfg(target_arch = "wasm32")]
    spawn(async move {
        let after = if more { cursor() } else { None };
        let after = crate::task_live::cursor_query(after.as_deref());
        let handle = js_sys::encode_uri_component(&handle)
            .as_string()
            .unwrap_or_default();
        match crate::task_live::get::<symphony_api::TaskListing>(&format!(
            "/api/task-board?holder={handle}&role=orchestrator&open=1&older=1{after}"
        ))
        .await
        {
            Ok(list) => {
                total.set(list.counts.iter().map(|(_, n)| n).sum());
                cursor.set(list.next);
                if !more {
                    rows.set(Vec::new());
                }
                for task in list.tasks {
                    if !rows.read().iter().any(|t| t.slug == task.slug) {
                        rows.write().push(task);
                    }
                }
            }
            Err(err) => trouble.set(err),
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (
            handle,
            more,
            &mut rows,
            &mut total,
            &mut cursor,
            &mut trouble,
        );
    }
}

#[cfg(test)]
#[path = "task_detail/tests.rs"]
mod tests;

/// What the owner has read of a long brief past the part its page sent, for one revision of the
/// brief: the chunks, the byte to read from next, the bytes still left and the last failure.
#[derive(Clone, Debug, Default, PartialEq)]
struct BriefRead {
    revision: String,
    blocks: Vec<symphony_api::Block>,
    next: usize,
    omitted: usize,
    failed: String,
}

impl BriefRead {
    /// Nothing read yet of the brief at `revision`, whose page stopped at `next` with `omitted` left.
    fn start(revision: &str, next: usize, omitted: usize) -> BriefRead {
        BriefRead {
            revision: revision.to_owned(),
            next,
            omitted,
            ..BriefRead::default()
        }
    }

    /// What was read of the brief at `revision`; `None` when what is held was read of another
    /// revision, which the brief's page no longer shows.
    fn of(&self, revision: &str) -> Option<&BriefRead> {
        (self.revision == revision).then_some(self)
    }

    /// `answer`, asked of the brief at `revision` (whose page stopped at `next` with `omitted`
    /// left), added after what was read of it. A chunk cut from another revision is not added: the
    /// brief changed after its page was sent, and the page reloads with the new one.
    fn add(
        &mut self,
        revision: &str,
        next: usize,
        omitted: usize,
        answer: Result<symphony_api::TaskBrief, String>,
    ) {
        if self.of(revision).is_none() {
            *self = BriefRead::start(revision, next, omitted);
        }
        match answer {
            Ok(chunk) if chunk.revision == revision => {
                self.blocks.extend(chunk.blocks);
                self.next = chunk.next;
                self.omitted = chunk.omitted;
                self.failed.clear();
            }
            Ok(_) => {
                self.failed = "The brief changed since this page was sent; the page shows the \
                               new one when it reloads."
                    .into();
            }
            Err(err) => self.failed = err,
        }
    }
}

/// The brief, which the page sends cut when it is very long; a button reads the rest, a chunk at a
/// time, after the part already shown. What was read belongs to one revision of the brief: an edit
/// starts again from the part the new page sent.
#[component]
fn Brief(
    slug: String,
    revision: String,
    blocks: Vec<symphony_api::Block>,
    omitted: usize,
    next: usize,
) -> Element {
    let mut read = use_signal(BriefRead::default);
    let mut busy = use_signal(|| false);
    let (more, from, remaining, failed) = match read.read().of(&revision) {
        Some(held) => (
            held.blocks.clone(),
            held.next,
            held.omitted,
            held.failed.clone(),
        ),
        None => (Vec::new(), next, omitted, String::new()),
    };
    let load = move |_| {
        if busy() {
            return;
        }
        busy.set(true);
        let (slug, revision) = (slug.clone(), revision.clone());
        spawn(async move {
            let answer = brief_chunk(&slug, from).await;
            read.write().add(&revision, next, omitted, answer);
            busy.set(false);
        });
    };
    let mut shown = blocks;
    shown.extend(more);
    rsx! { div { class: "task-body card", crate::block_view::BlocksView { blocks: shown }
        if remaining > 0 {
            p { class: "brief-cut", "This brief is long; {remaining} bytes of it are not shown yet. "
                crate::components::button::Button { id: "brief-more", disabled: busy(), onclick: load, "Show more of the brief" }
            }
            if !failed.is_empty() { p { role: "alert", "{failed}" } }
        }
    } }
}

/// The chunk of `slug`'s brief from byte `from`, as `/api/task-brief` answers it.
#[cfg(target_arch = "wasm32")]
async fn brief_chunk(slug: &str, from: usize) -> Result<symphony_api::TaskBrief, String> {
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    crate::task_live::get(&format!("/api/task-brief?slug={encoded}&from={from}")).await
}

/// The host build renders pages and has no fetch: only the browser reads a brief's chunks.
#[cfg(not(target_arch = "wasm32"))]
async fn brief_chunk(slug: &str, from: usize) -> Result<symphony_api::TaskBrief, String> {
    Err(format!(
        "this build cannot read the brief of {slug} from byte {from}"
    ))
}
