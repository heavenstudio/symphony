//! The execution providers a host binary links, installed beside its plugins
//! ([docs/EXECUTION_ATTEMPTS.md](../../../docs/EXECUTION_ATTEMPTS.md#on-a-tasks-page)): each the
//! adapter a project supplies for one provider configuration, with what it can do and the capacity
//! its attempts share. The server resumes an attempt and resolves a view of its resource only
//! through the provider its configuration names; a cancel or a stop it records for whichever
//! worker sends the attempt's operations, with or without one here.
//!
//! Nothing is discovered or loaded at run time: a binary installs its list before it serves, as
//! it installs its plugins ([`crate::binary::run_with_providers`]). The Symphony's own server
//! installs none.

use std::sync::{Arc, OnceLock};

use symphony_board::execution::{Adapter, Capabilities};

/// One provider configuration's adapter, as a host links it.
#[derive(Clone)]
pub struct Provider {
    configuration: String,
    capabilities: Capabilities,
    adapter: Arc<dyn Adapter>,
    capacity: Option<(String, u32)>,
}

impl Provider {
    /// `adapter`, answering for the attempts of `configuration`, offering `capabilities`.
    pub fn new(
        configuration: &str,
        capabilities: Capabilities,
        adapter: Arc<dyn Adapter>,
    ) -> Provider {
        Provider {
            configuration: configuration.to_string(),
            capabilities,
            adapter,
            capacity: None,
        }
    }

    /// The same provider, each attempt it admits (a resume here) reserving one of `limit` places
    /// in capacity scope `scope`: the scope and size its own worker admits under.
    #[must_use]
    pub fn with_capacity(mut self, scope: &str, limit: u32) -> Provider {
        self.capacity = Some((scope.to_string(), limit));
        self
    }

    pub fn configuration(&self) -> &str {
        &self.configuration
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub(crate) fn adapter(&self) -> &dyn Adapter {
        self.adapter.as_ref()
    }

    pub(crate) fn capacity(&self) -> Option<(&str, u32)> {
        self.capacity
            .as_ref()
            .map(|(scope, limit)| (scope.as_str(), *limit))
    }
}

/// The providers this process holds, by configuration.
#[derive(Clone, Default)]
pub(crate) struct Providers(Arc<Vec<Provider>>);

impl Providers {
    pub(crate) fn of(providers: Vec<Provider>) -> Providers {
        Providers(Arc::new(providers))
    }

    /// The installed ones, none when nothing was installed.
    pub(crate) fn installed() -> Providers {
        INSTALLED.get().cloned().unwrap_or_default()
    }

    /// The provider of `configuration`, if this process holds it.
    pub(crate) fn of_configuration(&self, configuration: &str) -> Option<&Provider> {
        self.0
            .iter()
            .find(|provider| provider.configuration == configuration)
    }
}

static INSTALLED: OnceLock<Providers> = OnceLock::new();

/// Makes `providers` the ones this process holds, for good: a host installs them before it serves.
/// Two under one configuration are refused, and so is a second install.
///
/// # Errors
///
/// When one configuration is named twice or is empty, or providers were installed already.
pub fn install(providers: Vec<Provider>) -> Result<(), String> {
    let mut named: Vec<&str> = providers.iter().map(Provider::configuration).collect();
    if named.iter().any(|name| name.trim().is_empty()) {
        return Err("an execution provider names its configuration".into());
    }
    named.sort_unstable();
    if let Some(pair) = named.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(format!(
            "two execution providers answer for configuration {:?}",
            pair[0]
        ));
    }
    INSTALLED
        .set(Providers::of(providers))
        .map_err(|_| "this process holds its execution providers already".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use symphony_board::execution::Outcome;
    use symphony_plugin::execution::{Authorization, Call, Unknown};

    struct Silent;

    impl Adapter for Silent {
        fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
            Outcome::Unknown(Unknown::ContactLost)
        }
    }

    fn provider(configuration: &str) -> Provider {
        Provider::new(configuration, Capabilities::default(), Arc::new(Silent))
    }

    #[test]
    fn a_provider_is_found_by_its_configuration_alone() {
        let held = Providers::of(vec![provider("cloud"), provider("lab").with_capacity("gpu", 2)]);
        assert_eq!(
            held.of_configuration("lab").and_then(Provider::capacity),
            Some(("gpu", 2))
        );
        assert_eq!(held.of_configuration("cloud").unwrap().capacity(), None);
        assert!(held.of_configuration("Cloud").is_none());
        assert!(Providers::default().of_configuration("cloud").is_none());
    }

    #[test]
    fn an_install_names_each_configuration_once_and_happens_once() {
        let twice = install(vec![provider("install-test"), provider("install-test")]).unwrap_err();
        assert!(twice.contains("\"install-test\""), "{twice}");
        let unnamed = install(vec![provider(" ")]).unwrap_err();
        assert!(unnamed.contains("names its configuration"), "{unnamed}");
        // Neither refusal installed anything; this process's tests install no other list.
        assert!(Providers::installed().of_configuration("install-test").is_none());
        install(vec![provider("install-test")]).unwrap();
        assert!(Providers::installed().of_configuration("install-test").is_some());
        let again = install(Vec::new()).unwrap_err();
        assert!(again.contains("already"), "{again}");
    }
}
