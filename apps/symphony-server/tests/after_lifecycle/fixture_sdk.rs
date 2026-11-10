//! Generic native lifecycle fixture; no containing-project recipe or policy input.
const FIXTURE_ROOT: &str = env!("CARGO_MANIFEST_DIR");
include!("fixture.rs");
use stand_ins::STAND_IN_LIFETIME;
impl Repo {
    pub fn new(name: &str) -> Self {
        Self::with_recipe(name, None)
    }
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod scratch_tests;

#[cfg(test)]
#[path = "descendant_cleanup_tests.rs"]
mod descendant_cleanup_tests;

#[cfg(test)]
#[path = "unsuccessful_child_tests.rs"]
mod unsuccessful_child_tests;

#[cfg(test)]
#[path = "fixture_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "fixture_children_tests.rs"]
mod children_tests;
