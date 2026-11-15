//! The agents page's views, compiled separately from the summary-only board.
use super::*;
use std::rc::Rc;
use symphony_api::Agents;

/// The agents page (`/agents`), live: its agents and the tasks they hold, as `task_live` keeps them.
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
        if data.read().agents.is_some() && *located.peek() != path {
            located.set(path);
            crate::navigation::scroll_anchor();
        }
    });
    // The page's error is drawn in its notice stack (`live::BoardPage`), out of the page's flow;
    // a page with nothing yet to show says in its body what failed.
    let ready = if data.read().drawn() {
        "agents"
    } else {
        crate::trouble::ready(false, !data.read().error.is_empty())
    };
    rsx! { div { class: "task-page", "data-ready": ready,
        if let Some(agents) = agents {
            AgentsView { agents, local: data.read().local, tasks: data.read().tasks.clone().unwrap_or_default(), total: data.read().agent_total, more: data.read().has_more_agents(false), lost_more: data.read().has_more_agents(true), onmore: move |lost| { spawn(task_live::more_agents(lost,route,data)); } }
        } else if !data.read().error.is_empty() {
            crate::trouble::Trouble { title: "The agents could not be loaded", text: crate::trouble::failed(&data.read().error), retry: move |_| { spawn(task_live::load(route, data)); } }
        } else { p { "Loading agents…" } }
    } }
}

/// An agent's Go to session control (docs/TASKS.md, "Going to a session"): an ordinary link to its
/// session's web address; for a page on the server's machine, a button that has the Herdr app
/// show its pane; for a page anywhere else, a button that shows the pane's screen, read only. A
/// subagent's goes to its parent's session and says so; with nothing known there is none.
#[component]
pub(crate) fn GoToSession(
    handle: String,
    go: Option<symphony_api::SessionGo>,
    local: bool,
) -> Element {
    use crate::components::button::{Button, ButtonSize};
    use symphony_api::SessionControl;
    let mut said = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut screen = use_signal(|| None::<symphony_api::AgentScreen>);
    let Some(go) = go else { return rsx! {} };
    let Some(control) = go.control(local) else {
        return rsx! {};
    };
    let parent = go.via_parent(&handle).then(|| go.handle.clone());
    let (kind, shown) = match control {
        // Only a `symphony_api::web_address` reaches here: `control` links nothing else.
        SessionControl::Link(url) => (
            "link",
            rsx! {
                a { class: "go-session-link", href: "{url}", target: "_blank", rel: "noopener noreferrer", "Go to session" }
            },
        ),
        SessionControl::Focus(pane) => (
            "focus",
            rsx! {
                Button { r#type: "button", size: ButtonSize::Xs, disabled: busy(), title: "Shows {pane} in the Herdr app on this machine",
                    onclick: move |_| {
                        if busy() { return; }
                        busy.set(true);
                        said.set(String::new());
                        let handle = handle.clone();
                        spawn(async move {
                            #[cfg(target_arch = "wasm32")]
                            match crate::task_live::post(symphony_api::AGENT_FOCUS_PATH, &[("handle", &handle)]).await {
                                Ok(answer) => said.set(serde_wasm_bindgen::from_value::<symphony_api::AgentFocus>(answer).map_or_else(|e| e.to_string(), |focus| focus.said)),
                                Err(err) => said.set(err),
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            { let _ = handle; }
                            busy.set(false);
                        });
                    },
                    "Go to session"
                }
            },
        ),
        SessionControl::Screen(pane) => (
            "screen",
            rsx! {
                Button { r#type: "button", size: ButtonSize::Xs, disabled: busy(), title: "Shows {pane}'s screen, read only",
                    onclick: move |_| {
                        if busy() { return; }
                        busy.set(true);
                        said.set(String::new());
                        let handle = handle.clone();
                        spawn(async move {
                            #[cfg(target_arch = "wasm32")]
                            {
                                let encoded = js_sys::encode_uri_component(&handle).as_string().unwrap_or_default();
                                match crate::task_live::get::<symphony_api::AgentScreen>(&format!("{}?handle={encoded}", symphony_api::AGENT_SCREEN_PATH)).await {
                                    Ok(read) => screen.set(Some(read)),
                                    Err(err) => said.set(err),
                                }
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            { let _ = handle; }
                            busy.set(false);
                        });
                    },
                    if screen().is_some() { "Read the screen again" } else { "Go to session" }
                }
            },
        ),
    };
    rsx! { span { class: "go-session", "data-go": kind,
        {shown}
        if let Some(parent) = parent { span { class: "go-session-via", "in {parent}'s session" } }
        if !said().is_empty() { span { role: "status", "{said}" } }
        if let Some(read) = screen() {
            span { class: "session-screen", role: "region", aria_label: "{read.pane} in {read.handle}'s session, read only",
                pre { "{read.text}" }
                Button { r#type: "button", size: ButtonSize::Xs, onclick: move |_| screen.set(None), "Close" }
            }
        }
    } }
}

#[component(lazy)]
pub fn AgentsView(
    mut agents: Agents,
    #[props(default)] local: bool,
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
            Near { key: "{agent.handle}", id: "agent-{agent.handle}", height: 240, AgentRow { agent: agent.clone(), local, tasks: tasks.clone(), work: work.clone() } }
        } }
        if more && agents.agents.iter().filter(|a| a.health != "lost" && a.health != "left").count() < total { Near { id: "more-agents".to_string(), onenter: move |_| onmore.call(false), crate::components::button::Button { onclick: move |_| onmore.call(false), "Load more agents" } } }
        details { summary { onclick: move |_| onmore.call(true), "Lost agents" } div { for agent in agents.agents.into_iter().filter(|a| a.health == "lost") { Near { key: "{agent.handle}", id: "agent-{agent.handle}", height: 120, AgentRow { agent, local, tasks: tasks.clone(), work: work.clone() } } } if lost_more { crate::components::button::Button { onclick: move |_| onmore.call(true), "Load more lost agents" } } } }
    }
}

#[component]
fn AgentRow(
    agent: symphony_api::Agent,
    #[props(default)] local: bool,
    tasks: Rc<Vec<Task>>,
    work: SharedWork,
) -> Element {
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
    let followed = followed_tasks(following_rows(), &tasks, &agent.handle);
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
        p { class: "muted", {meta(&[&agent.kind, &agent.model, &agent.state, &agent.branch, agent.worktree.rsplit('/').next().unwrap_or(&agent.worktree)])} }
        GoToSession { handle: agent.handle.clone(), go: agent.go.clone(), local }
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
        // A dot between two finished tasks, none after the last.
        if !finished.is_empty() { p { class: "muted", "Finished: " for (at, (task, _)) in finished.iter().take(5).enumerate() { if at > 0 { " · " } a { href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" } } if finished.len() > 5 { " · and {finished.len() - 5} more" } } }
        if let Some(said) = agent.last_message { p { a { href: crate::project::href(&format!("/task/{}", said.slug)), "Latest message" } ": {said.text}" } }
    } }
}

/// The open tasks `handle` orchestrates, as its Following list shows them: the rows the server
/// paged (`rows`), each as the board now knows it, and any other the board shows it orchestrating,
/// in the board's one order (`by_priority`: priority, then slug with its numbers by value). The
/// server pages them in that order, so the rows Load more brings follow the ones already shown.
fn followed_tasks(rows: Vec<Task>, tasks: &[Task], handle: &str) -> Vec<Task> {
    let open = |t: &Task| t.status != "done" && t.status != "cancelled";
    let mut followed: Vec<_> = rows
        .into_iter()
        .map(|row| {
            tasks
                .iter()
                .find(|t| t.slug == row.slug)
                .cloned()
                .unwrap_or(row)
        })
        .filter(|t| open(t))
        .collect();
    for task in tasks.iter().filter(|t| {
        open(t)
            && t.roles
                .iter()
                .any(|r| r.handle == handle && r.role == "orchestrator")
    }) {
        if !followed.iter().any(|t| t.slug == task.slug) {
            followed.push(task.clone());
        }
    }
    followed.sort_by(|a, b| by_priority(&a, &b));
    followed
}

/// An agent's line of what is known of it, its kind, model, state, branch and worktree: the parts
/// it has, a dot between two, so an empty one leaves no gap.
fn meta(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
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
