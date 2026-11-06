//! The quick-task composer keeps an uncertain save's identity and text across a reload.

use super::{SHOWN, chrome, components, site};
use serde_json::json;

#[test]
fn an_uncertain_task_save_keeps_its_draft_and_identity_until_confirmation() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    site.set_json("/api/quick-task", &json!({"slug":"inbox/first"}));
    page.open(&site.url("/"));
    components::open_panel(&mut page);
    components::task_tab(&mut page);
    page.eval("for(const [id,value] of [['quick-title','First task'],['quick-text','First details']]) { const field=document.getElementById(id);field.value=value;field.dispatchEvent(new Event('input',{bubbles:true})); }");
    page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
    assert!(page.wait_for("document.querySelector('#quick-sheet a[href=\"/task/inbox/first\"]') !== null && document.getElementById('quick-title').value===''", SHOWN));
    site.set_json(
        "/api/quick-task",
        &json!({"pending":true,"error":"outcome not known yet"}),
    );
    page.eval("for(const [id,value] of [['quick-title','Second task'],['quick-text','Keep these typed details']]) { const field=document.getElementById(id);field.value=value;field.dispatchEvent(new Event('input',{bubbles:true})); }");
    page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet').textContent.includes('Checking save…')",
        SHOWN
    ));
    assert_eq!(page.eval("({title:document.getElementById('quick-title').value,text:document.getElementById('quick-text').value,locked:document.getElementById('quick-title').disabled,oldSuccess:!!document.querySelector('#quick-sheet a[href=\"/task/inbox/first\"]')})"), json!({"title":"Second task","text":"Keep these typed details","locked":true,"oldSuccess":false}));
    let posts = site.posts();
    let original = posts
        .iter()
        .find(|(path, body)| path == "/api/quick-task" && body["title"] == "Second task")
        .unwrap()
        .1
        .clone();
    let request = original["request"].as_str().unwrap();
    assert_eq!(request.len(), 32);
    assert_eq!(
        page.eval("JSON.parse(localStorage.getItem('review-quick-draft')).request"),
        json!(request)
    );

    // A different page on reload must still check the original payload and origin.
    page.eval("setTimeout(() => location.assign('/tasks'), 0)");
    let dialogs = page.wait_for_protocol_events("Page.javascriptDialogOpening", 1, SHOWN);
    assert_eq!(
        dialogs.len(),
        1,
        "leaving an active save requires confirmation"
    );
    assert_eq!(dialogs[0]["type"], "beforeunload");
    page.call("Page.handleJavaScriptDialog", json!({"accept":true}));
    assert!(page.wait_for(
        "location.pathname==='/tasks' && document.documentElement?.dataset.navigation==='ready'",
        SHOWN
    ));
    components::open_panel(&mut page);
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet button[type=submit]')?.textContent==='Check save'",
        SHOWN
    ));
    assert_eq!(page.eval("[document.getElementById('quick-title').value,document.getElementById('quick-text').value]"), json!(["Second task","Keep these typed details"]));
    site.fail_route("/api/quick-task", true);
    page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet').textContent.includes('Checking save…')",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.getElementById('quick-text').value"),
        json!("Keep these typed details")
    );
    site.set_json("/api/quick-task", &json!({"slug":"inbox/second"}));
    site.fail_route("/api/quick-task", false);
    assert!(page.wait_for("document.querySelector('#quick-sheet a[href=\"/task/inbox/second\"]') !== null && document.getElementById('quick-title').value==='' && document.getElementById('quick-text').value===''", SHOWN));
    let posts = site.posts();
    let checks: Vec<_> = posts
        .iter()
        .filter(|(path, body)| path == "/api/quick-task" && body["title"] == "Second task")
        .collect();
    assert!(
        checks.len() >= 3,
        "pending, unavailable, then confirmed: {} checks",
        checks.len()
    );
    assert!(
        checks.iter().all(|(_, body)| body == &original),
        "every check must use the original identity and payload: {checks:?}"
    );

    known_refusals(&mut page);
}

fn known_refusals(page: &mut super::chrome::Chrome) {
    page.call(
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*api/quick-task","requestStage":"Request"}]}),
    );
    page.eval("for(const [id,value] of [['quick-title','Third task'],['quick-text','Keep the rejected text']]) { const field=document.getElementById(id);field.value=value;field.dispatchEvent(new Event('input',{bubbles:true})); }");
    page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
    let held = page.wait_for_protocol_events("Fetch.requestPaused", 1, SHOWN);
    assert_eq!(held.len(), 1);
    let body = page.eval("btoa(JSON.stringify({error:'Task slug already exists'}))");
    page.call("Fetch.fulfillRequest", json!({"requestId":held[0]["requestId"],"responseCode":400,"responseHeaders":[{"name":"Content-Type","value":"application/json"}],"body":body}));
    assert!(page.wait_for("document.querySelector('#quick-sheet [role=alert]')?.textContent==='Task slug already exists'", SHOWN));
    assert_eq!(page.eval("({title:document.getElementById('quick-title').value,text:document.getElementById('quick-text').value,locked:document.getElementById('quick-title').disabled,oldSuccess:!!document.querySelector('#quick-sheet a[href=\"/task/inbox/second\"]')})"), json!({"title":"Third task","text":"Keep the rejected text","locked":false,"oldSuccess":false}));
    page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
    // The browser harness retains the first refusal's event in its protocol history.
    let held = page.wait_for_protocol_events("Fetch.requestPaused", 2, SHOWN);
    assert_eq!(held.len(), 2);
    assert_ne!(held[0]["requestId"], held[1]["requestId"]);
    let body = page.eval("btoa(JSON.stringify({error:'Board unavailable before dispatch'}))");
    page.call("Fetch.fulfillRequest", json!({"requestId":held[1]["requestId"],"responseCode":422,"responseHeaders":[{"name":"Content-Type","value":"application/json"}],"body":body}));
    assert!(page.wait_for("document.querySelector('#quick-sheet [role=alert]')?.textContent==='Board unavailable before dispatch'", SHOWN));
    assert_eq!(page.eval("({text:document.getElementById('quick-text').value,locked:document.getElementById('quick-title').disabled})"), json!({"text":"Keep the rejected text","locked":false}));
    page.call("Fetch.disable", json!({}));
}
