//! A close is a barrier: no close returns while the verdict filing worker still runs, however
//! many close at once.

use super::*;
use std::time::Duration;

/// Saves with no committer, whose filing worker runs until `released` gets a message (or its
/// sender goes) and then marks `finished`.
fn filing_until(released: mpsc::Receiver<()>, finished: Arc<AtomicBool>) -> Saves {
    let worker = std::thread::spawn(move || {
        let _ = released.recv();
        finished.store(true, Ordering::Release);
    });
    Saves {
        root: PathBuf::new(),
        writes: Arc::new(Mutex::new(())),
        taps: Mutex::new(VecDeque::new()),
        unpublished: Mutex::new(BTreeMap::new()),
        committer: OnceLock::new(),
        closed: AtomicBool::new(false),
        filer: Mutex::new(None),
        filing_worker: Mutex::new(Some(worker)),
    }
}

#[test]
fn concurrent_closes_both_wait_for_the_live_filing_worker() {
    let (release, released) = mpsc::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let saves = Arc::new(filing_until(released, finished.clone()));
    let (filing, at_filing) = mpsc::channel();
    let (returned, came_back) = mpsc::channel();
    let close = || {
        let (saves, finished) = (saves.clone(), finished.clone());
        let (filing, returned) = (filing.clone(), returned.clone());
        std::thread::spawn(move || {
            saves.close_saying(|stage| {
                if stage == "the verdict filing" {
                    let _ = filing.send(());
                }
            });
            // Whether the worker had ended when this close returned.
            returned.send(finished.load(Ordering::Acquire)).unwrap();
        })
    };
    let first = close();
    at_filing.recv().unwrap();
    let second = close();
    at_filing.recv().unwrap();
    // Both closes have reached the filing while the worker still runs. This bounded wait is a
    // negative assertion, never the synchronization: neither close may return before release.
    let early = came_back.recv_timeout(Duration::from_millis(500));
    assert!(
        early.is_err(),
        "a close returned before the filing worker ended (worker ended: {early:?})"
    );
    release.send(()).unwrap();
    let ended: Vec<bool> = (0..2)
        .map(|_| came_back.recv_timeout(Duration::from_secs(30)).unwrap())
        .collect();
    assert_eq!(ended, [true, true], "each close saw the worker ended");
    first.join().unwrap();
    second.join().unwrap();
}

#[test]
fn a_close_after_a_close_names_the_filing_and_returns() {
    let (release, released) = mpsc::channel::<()>();
    let finished = Arc::new(AtomicBool::new(false));
    let saves = filing_until(released, finished.clone());
    drop(release);
    saves.close();
    assert!(finished.load(Ordering::Acquire));
    let named = Mutex::new(Vec::new());
    saves.close_saying(|stage| named.lock().unwrap().push(stage.to_owned()));
    assert_eq!(
        named.into_inner().unwrap(),
        ["the save in flight", "the verdict filing"]
    );
}
