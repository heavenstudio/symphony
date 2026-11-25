use super::{place, sort_by, sort_by_key};

fn sort<T: Ord>(items: &mut [T]) {
    sort_by(items, &T::cmp);
}
use std::cmp::Ordering;

/// `count` pairs of a key among `keys` and the position each was made at, out of order: a key
/// given many times, so a stable sort is told from an unstable one by the positions.
fn pairs(count: usize, keys: u32) -> Vec<(u32, usize)> {
    let mut state: u32 = 0x9e37_79b9;
    (0..count)
        .map(|made| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state % keys, made)
        })
        .collect()
}

#[test]
fn a_sort_by_is_the_standard_librarys_stable_sort_by() {
    for (count, keys) in [
        (0, 1),
        (1, 1),
        (2, 2),
        (7, 3),
        (20, 2),
        (64, 5),
        (1000, 30),
        (1000, 1000),
    ] {
        let by_key = |a: &(u32, usize), b: &(u32, usize)| a.0.cmp(&b.0);
        let mut ours = pairs(count, keys);
        let mut theirs = ours.clone();
        sort_by(&mut ours, &by_key);
        theirs.sort_by(by_key);
        assert_eq!(ours, theirs, "{count} pairs of {keys} keys");
    }
}

#[test]
fn equal_elements_keep_the_order_they_stood_in() {
    let mut ours = pairs(500, 4);
    sort_by_key(&mut ours, &|pair| pair.0);
    for both in ours.windows(2) {
        let ((a, made_a), (b, made_b)) = (both[0], both[1]);
        assert!(a < b || (a == b && made_a < made_b), "{both:?}");
    }
}

#[test]
fn a_comparison_that_reverses_sorts_in_reverse_stably() {
    let mut ours = pairs(300, 6);
    let mut theirs = ours.clone();
    let descending = |a: &(u32, usize), b: &(u32, usize)| b.0.cmp(&a.0);
    sort_by(&mut ours, &descending);
    theirs.sort_by(descending);
    assert_eq!(ours, theirs);
}

#[test]
fn a_sort_by_key_and_a_sort_are_the_standard_librarys() {
    let mut keyed = pairs(200, 9);
    let mut expected = keyed.clone();
    sort_by_key(&mut keyed, &|pair| std::cmp::Reverse(pair.0));
    expected.sort_by_key(|pair| std::cmp::Reverse(pair.0));
    assert_eq!(keyed, expected);

    let mut words: Vec<String> = ["pear", "apple", "fig", "apple", "", "Fig", "banana"]
        .map(String::from)
        .to_vec();
    let mut sorted = words.clone();
    sort(&mut words);
    sorted.sort();
    assert_eq!(words, sorted);
}

#[test]
fn sorted_and_reversed_input_and_a_slice_of_a_vec() {
    let mut ascending: Vec<u32> = (0..100).collect();
    sort(&mut ascending);
    assert_eq!(ascending, (0..100).collect::<Vec<_>>());

    let mut descending: Vec<u32> = (0..100).rev().collect();
    sort(&mut descending);
    assert_eq!(descending, (0..100).collect::<Vec<_>>());

    let mut part = vec![9, 3, 7, 1, 5, 0];
    sort(&mut part[1..5]);
    assert_eq!(part, [9, 1, 3, 5, 7, 0]);
}

#[test]
fn floats_by_total_order() {
    let mut ours = vec![2.5, -0.0, f64::NAN, 0.0, -1.0, f64::INFINITY, 2.5];
    let mut theirs = ours.clone();
    sort_by(&mut ours, &f64::total_cmp);
    theirs.sort_by(f64::total_cmp);
    let bits = |values: &[f64]| values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&ours), bits(&theirs));
}

#[test]
fn place_moves_each_element_along_its_cycle_once() {
    // Two cycles (0 2 4) and (1 3), and 5 left where it is.
    let mut items = ['a', 'b', 'c', 'd', 'e', 'f'];
    let mut order = [2, 3, 4, 1, 0, 5];
    place(&mut items, &mut order);
    assert_eq!(items, ['c', 'd', 'e', 'b', 'a', 'f']);
    assert_eq!(order, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn no_comparison_is_made_of_fewer_than_two_elements() {
    let refuse = |_: &u8, _: &u8| -> Ordering { panic!("nothing to compare") };
    let mut none: [u8; 0] = [];
    sort_by(&mut none, &refuse);
    let mut one = [7];
    sort_by(&mut one, &refuse);
    assert_eq!(one, [7]);
}

#[test]
fn a_panicking_comparison_keeps_every_item_in_its_original_place() {
    let mut items = pairs(80, 9);
    let original = items.clone();
    let comparisons = std::cell::Cell::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sort_by(&mut items, &|a, b| {
            comparisons.set(comparisons.get() + 1);
            assert_ne!(comparisons.get(), 12, "comparison failed");
            a.0.cmp(&b.0)
        });
    }));
    assert!(result.is_err());
    assert_eq!(items, original);
}

#[test]
fn a_sort_moves_owning_values_without_losing_or_dropping_them() {
    struct Item(u32, std::rc::Rc<std::cell::Cell<usize>>);
    impl Drop for Item {
        fn drop(&mut self) {
            self.1.set(self.1.get() + 1);
        }
    }
    let dropped = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut items: Vec<_> = (0..80).rev().map(|i| Item(i, dropped.clone())).collect();
    sort_by_key(&mut items, &|item| item.0);
    assert_eq!(
        items.iter().map(|item| item.0).collect::<Vec<_>>(),
        (0..80).collect::<Vec<_>>()
    );
    assert_eq!(dropped.get(), 0);
    drop(items);
    assert_eq!(dropped.get(), 80);
}

#[test]
fn zero_sized_items_are_kept() {
    let mut items = [(); 80];
    sort_by(&mut items, &|_, _| Ordering::Equal);
    assert_eq!(items, [(); 80]);
}

/// What an unstable sort taken to this one sorts: elements whose equal ones cannot be told apart
/// (strings, numbers, paths, pairs of those), so the order of equals the stable sort gives is the
/// only order there is, and the result is the unstable sort's.
#[test]
fn where_equal_elements_are_alike_the_sort_is_the_unstable_sorts() {
    let words: Vec<String> = ["b", "a", "", "ab", "a", "b", "B"]
        .map(String::from)
        .to_vec();
    let mut ours = words.clone();
    let mut theirs = words;
    sort_by(&mut ours, &Ord::cmp);
    theirs.sort_unstable();
    assert_eq!(ours, theirs);
    // Newest first, as a folder of dated logs is read.
    sort_by(&mut ours, &|a: &String, b: &String| b.cmp(a));
    theirs.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(ours, theirs);

    let mut ports: Vec<u16> = vec![8080, 22, 8080, 1, 65535, 22, 0];
    let mut sorted = ports.clone();
    sort_by(&mut ports, &Ord::cmp);
    sorted.sort_unstable();
    assert_eq!(ports, sorted);

    let names = ["7.place".to_string(), "10.place".into(), "7.place".into()];
    let mut numbered: Vec<(u64, &String)> = vec![(10, &names[1]), (7, &names[0]), (7, &names[2])];
    let mut sorted = numbered.clone();
    sort_by(&mut numbered, &Ord::cmp);
    sorted.sort_unstable();
    assert_eq!(numbered, sorted);

    let mut paths: Vec<std::path::PathBuf> = ["b/x.log", "a.log", "b/x.log", "a"]
        .map(std::path::PathBuf::from)
        .to_vec();
    let mut sorted = paths.clone();
    sort_by(&mut paths, &Ord::cmp);
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
}
