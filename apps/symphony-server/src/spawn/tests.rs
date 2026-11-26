use super::{blocking, task};
use test_time::wait::{FLOOR, until};

#[tokio::test]
async fn the_answer_is_what_the_work_returned() {
    assert_eq!(blocking(|| 6 * 7).await.unwrap(), 42);
    assert_eq!(
        blocking(|| String::from("read")).await.unwrap(),
        "read".to_string()
    );
    let answer: Result<(), String> = blocking(|| Err("refused".into())).await.unwrap();
    assert_eq!(answer, Err("refused".to_string()));
}

#[tokio::test]
async fn the_work_runs_on_the_blocking_pool_not_the_calling_thread() {
    let caller = std::thread::current().id();
    let ran_on = blocking(|| std::thread::current().id()).await.unwrap();
    assert_ne!(ran_on, caller);
}

#[tokio::test]
async fn a_work_that_panics_answers_its_panic() {
    let ended = blocking(|| -> u8 { panic!("the work broke") }).await;
    let error = ended.unwrap_err();
    assert!(error.is_panic(), "{error}");
}

#[tokio::test]
async fn calls_at_once_each_get_their_own_answer() {
    let calls: Vec<_> = (0..16_u32).map(|n| blocking(move || n * 10)).collect();
    let mut answers = Vec::new();
    for call in calls {
        answers.push(call.await.unwrap());
    }
    assert_eq!(answers, (0..16).map(|n| n * 10).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_call_dropped_before_its_end_still_runs_its_work_to_the_end() {
    let (started, wait_start) = crate::test_channel::channel();
    let (go, wait_go) = crate::test_channel::channel::<()>();
    let (ended, wait_end) = crate::test_channel::channel();
    let call = blocking(move || {
        started.send(()).unwrap();
        wait_go.recv().unwrap();
        ended.send(()).unwrap();
        "dropped with the call"
    });
    until("the work to start", FLOOR, || wait_start.try_recv().ok()).unwrap();
    drop(call);
    go.send(()).unwrap();
    until("the dropped call's work to end", FLOOR, || {
        wait_end.try_recv().ok()
    })
    .unwrap();
}

#[tokio::test]
async fn a_spawned_task_runs_and_its_handle_ends_with_it() {
    let (said, heard) = tokio::sync::oneshot::channel();
    let handle = task(async move {
        said.send("ran").unwrap();
    });
    handle.await.unwrap();
    assert_eq!(heard.await.unwrap(), "ran");
}

#[tokio::test]
async fn a_spawned_task_that_panics_ends_its_handle_with_the_panic() {
    let error = task(async { panic!("the task broke") }).await.unwrap_err();
    assert!(error.is_panic(), "{error}");
}
