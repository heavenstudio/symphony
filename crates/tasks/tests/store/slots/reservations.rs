//! A cap refusal grants no reservation: acquire or renew it, and stop when another holder wins.

use super::{HOLDS_EIGHT, K, O, W, capped, eight_and, reserve_hint};
use crate::common::{NOW, refused};

#[test]
fn a_reservation_can_be_acquired_and_renewed_at_the_cap_until_a_slot_frees() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    let full = format!(
        "work/next cannot start: {HOLDS_EIGHT}{}",
        reserve_hint("work/next")
    );
    assert_eq!(refused(store.claim("work/next", K)), full);
    let next = store.get("work/next").unwrap();
    assert!(next.holders("reserved").is_empty(), "{next:?}");

    let reserved = store.reserve("work/next", K, 60.0).unwrap();
    assert_eq!(reserved.holders("reserved"), [K]);
    assert_eq!(reserved.status, "ready");
    assert_eq!(refused(store.claim("work/next", K)), full);
    assert_eq!(store.get("work/next").unwrap().holders("reserved"), [K]);

    f.advance(30.0);
    let renewed = store.reserve("work/next", K, 60.0).unwrap();
    let role = renewed.roles.iter().find(|r| r.role == "reserved").unwrap();
    assert_eq!((role.handle.as_str(), role.expires), (K, Some(NOW + 90.0)));
    assert_eq!(refused(store.claim("work/next", K)), full);
    store.move_to("work/b", "review", O).unwrap();
    let claimed = store.claim("work/next", K).unwrap();
    assert_eq!(claimed.status, "in_progress");
    assert_eq!(claimed.holders("executor"), [K]);
    assert!(claimed.holders("reserved").is_empty(), "{claimed:?}");
    assert_eq!(
        f.said("work/next"),
        ["kite reserved it", "kite reserved it", "kite claimed it"]
    );
}

#[test]
fn an_expired_reservation_is_not_restored_by_a_cap_refusal_and_another_holder_stops_renewal() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    store.reserve("work/next", K, 60.0).unwrap();
    f.advance(61.0);
    let expired = store.get("work/next").unwrap();
    assert!(expired.holders("reserved").is_empty(), "{expired:?}");
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {HOLDS_EIGHT}{}",
            reserve_hint("work/next")
        )
    );
    let expired = store.get("work/next").unwrap();
    assert!(expired.holders("reserved").is_empty(), "{expired:?}");
    assert_eq!(
        store
            .reserve("work/next", W, 60.0)
            .unwrap()
            .holders("reserved"),
        [W]
    );
    assert_eq!(
        refused(store.reserve("work/next", K, 60.0)),
        "work/next is reserved by wren"
    );
    assert_eq!(
        refused(store.claim("work/next", K)),
        "work/next is reserved by wren"
    );
    let task = store.get("work/next").unwrap();
    assert_eq!(task.status, "ready");
    assert_eq!(task.holders("reserved"), [W]);
    assert!(task.holders("executor").is_empty(), "{task:?}");
    assert_eq!(
        f.said("work/next"),
        ["kite reserved it", "wren reserved it"]
    );
}

#[test]
fn holder_reservation_and_dependency_refusals_at_the_cap_give_no_advice_to_start_work() {
    for (kind, expected) in [
        ("held", "work/next is held by wren (live)"),
        ("reserved", "work/next is reserved by wren"),
        ("dependency", "work/next waits on work/a"),
    ] {
        let f = eight_and(&[("work/next", "ready")]);
        match kind {
            "held" => {
                f.store.claim("work/next", W).unwrap();
            }
            "reserved" => {
                f.store.reserve("work/next", W, 60.0).unwrap();
            }
            _ => {
                f.store
                    .change_depends("work/next", &["work/a".into()], &[], O)
                    .unwrap();
            }
        }
        let before = f.said("work/next");
        assert_eq!(
            refused(capped(&f, 8).claim("work/next", K)),
            expected,
            "{kind}"
        );
        assert_eq!(
            f.said("work/next"),
            before,
            "{kind}: refusal writes nothing"
        );
    }
}
