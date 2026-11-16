//! The entry of the retired client's dev server, kept by name alone: a consuming project's host
//! still calls [`main`] from a binary of its own (`tests/consumer_api.rs` holds its shape), and a
//! release changes none of a consumer's code, so the name goes only once that binary has.
//! It starts nothing and refuses, naming what took its place: the web client's dev server
//! (`apps/symphony-web/dev.js`, the recipe `symphony-ui-dev SERVER [PORT]`).

/// What [`main`] says: the dev server it started is gone, and where its successor is.
pub const RETIRED: &str = "symphony-ui-dev: the dev server this started went with the client it \
                           built; the web client's is the recipe `symphony-ui-dev SERVER [PORT]` \
                           of the Symphony's justfile (apps/symphony-web/dev.js)";

/// Refuses, whatever `args` are, with [`RETIRED`]: the name a consumer's binary calls, which
/// builds on this release as it did on the last.
///
/// # Errors
/// Always, with the line that names the replacement.
pub fn main(_args: &[String]) -> Result<(), String> {
    Err(RETIRED.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retired_entry_refuses_whatever_it_is_handed_and_names_its_replacement() {
        for args in [
            Vec::new(),
            vec!["root".to_owned(), "dx".to_owned(), "0".to_owned()],
        ] {
            let said = main(&args).unwrap_err();
            assert_eq!(said, RETIRED);
            assert_eq!(said.lines().count(), 1, "one line: {said}");
            assert!(said.contains("symphony-ui-dev SERVER [PORT]"), "{said}");
            assert!(said.contains("apps/symphony-web/dev.js"), "{said}");
        }
    }
}
