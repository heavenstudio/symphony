//! A deterministic provider: resources in memory, keyed by the operation keys that made them, so
//! a repeated key converges on what it made. Faults are scripted call by call ([`Fault`]): a lost
//! response, an outage, a crash before or after the provider acted, a refusal or a conflict; and
//! [`Meanwhile`] runs what happens while a call is in flight (a second worker, a cancel). It makes
//! no paid call and keeps no time: every wait is the test's own step.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;
use symphony_board::execution::{
    Adapter, Capabilities, ExternalLink, Operation, Outcome, ResumeMode, StopMode,
};
use symphony_plugin::execution::{
    Authorization, Call, Observation, Receipt, Rejection, Request, Resource, ResourceId,
    RuntimeState, Unknown,
};

/// What the next call does instead of answering plainly.
pub enum Fault {
    /// The provider acts, and its answer is lost: the caller hears contact lost.
    Lost,
    /// The provider is not reached: nothing happens, and the caller hears contact lost.
    Down,
    /// The provider refuses it.
    Refuse(&'static str),
    /// A reconcile finds two resources for the key.
    Twice,
    /// The worker dies before the provider is reached.
    CrashBefore,
    /// The provider acts, then the worker dies before it hears.
    CrashAfter,
}

/// What the provider holds.
#[derive(Default)]
struct Held {
    /// The resource each operation key made.
    made: HashMap<String, Resource>,
    /// Each resource's runtime and how many observations it has answered.
    runtimes: HashMap<String, (RuntimeState, u64)>,
    /// The operation and key of every call that reached it.
    calls: Vec<(Operation, String)>,
    faults: VecDeque<Fault>,
    resources: usize,
}

#[derive(Default)]
pub struct Fake {
    held: Mutex<Held>,
}

/// Every operation, stop and resume mode, an authoritative absence, ordered cursors and a view.
pub fn capabilities() -> Capabilities {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![
        Operation::Start,
        Operation::Reconcile,
        Operation::Observe,
        Operation::Cancel,
        Operation::Stop,
        Operation::Resume,
        Operation::ExternalLink,
    ];
    capabilities.stop_modes = vec![StopMode::Pause, StopMode::Terminate];
    capabilities.resume_modes = vec![ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration];
    capabilities.authoritative_absence = true;
    capabilities.ordered_observations = true;
    capabilities.external_view = true;
    capabilities
}

impl Fake {
    /// The next calls, in order, each meet one of `faults`.
    pub fn script(&self, faults: impl IntoIterator<Item = Fault>) {
        self.held.lock().unwrap().faults.extend(faults);
    }

    /// The resource `id`'s runtime becomes `state`, as the provider would report it next.
    pub fn runtime(&self, id: &str, state: RuntimeState) {
        let mut held = self.held.lock().unwrap();
        held.runtimes
            .get_mut(id)
            .expect("a resource the provider made")
            .0 = state;
    }

    pub fn runtime_of(&self, id: &str) -> RuntimeState {
        self.held.lock().unwrap().runtimes[id].0
    }

    /// How many resources the provider made.
    pub fn resources(&self) -> usize {
        self.held.lock().unwrap().resources
    }

    /// The operations that reached the provider, in order.
    pub fn calls(&self) -> Vec<Operation> {
        let held = self.held.lock().unwrap();
        held.calls.iter().map(|(operation, _)| *operation).collect()
    }

    /// The keys of the calls of `operation` that reached it, in order.
    pub fn keys(&self, operation: Operation) -> Vec<String> {
        let held = self.held.lock().unwrap();
        (held.calls.iter())
            .filter(|(made, _)| *made == operation)
            .map(|(_, key)| key.clone())
            .collect()
    }

    /// `call` done, as the provider does it.
    fn act(&self, call: &Call) -> Outcome {
        let mut held = self.held.lock().unwrap();
        let id = &call.identity;
        held.calls.push((call.request.operation(), id.key.clone()));
        let receipt = |resource: &Resource| {
            Receipt::new(
                id.clone(),
                resource.clone(),
                format!("receipt of {}", id.key),
            )
        };
        match &call.request {
            Request::Start { .. } | Request::Resume { .. } => {
                if let Some(made) = held.made.get(&id.key) {
                    return Outcome::Accepted(receipt(made));
                }
                held.resources += 1;
                let (name, generation) = match &call.request {
                    Request::Resume {
                        from,
                        mode: ResumeMode::ReuseWithNewGeneration,
                        ..
                    } => (
                        from.resource.id.value().clone(),
                        format!("{}+", from.resource.generation),
                    ),
                    _ => (format!("res-{}", held.resources), "g1".to_string()),
                };
                let resource = Resource::new(
                    id.attempt.clone(),
                    id.configuration.clone(),
                    ResourceId::new(name.clone()),
                    generation,
                );
                held.made.insert(id.key.clone(), resource.clone());
                held.runtimes.insert(name, (RuntimeState::Running, 0));
                Outcome::Accepted(receipt(&resource))
            }
            Request::Reconcile { .. } | Request::ReconcileResume { .. } => {
                match held.made.get(&id.key) {
                    Some(made) => Outcome::Found(receipt(made)),
                    None => Outcome::Absent {
                        evidence: format!("no resource under {}", id.key),
                    },
                }
            }
            Request::Observe { resource, .. } => {
                let runtime = held
                    .runtimes
                    .get_mut(resource.id.value())
                    .expect("an observed resource the provider made");
                runtime.1 += 1;
                let mut observation = Observation::new(resource.clone(), runtime.0);
                observation.cursor = Some(format!("{:06}", runtime.1));
                observation.conversation = Some(symphony_plugin::execution::ConversationId::new(
                    format!("conversation-of-{}", resource.id.value()),
                ));
                Outcome::Observed(observation)
            }
            Request::Cancel { resource } => {
                held.runtimes.get_mut(resource.id.value()).unwrap().0 = RuntimeState::Terminated;
                Outcome::Accepted(receipt(resource))
            }
            Request::Stop { resource, mode } => {
                held.runtimes.get_mut(resource.id.value()).unwrap().0 = match mode {
                    StopMode::Pause => RuntimeState::Paused,
                    _ => RuntimeState::Terminated,
                };
                Outcome::Accepted(receipt(resource))
            }
            Request::ExternalLink { resource, .. } => Outcome::Link(ExternalLink::new(
                resource.clone(),
                format!("https://provider.invalid/{}", resource.id.value()),
                10_000,
            )),
            _ => Outcome::Unsupported(call.request.operation()),
        }
    }
}

impl Adapter for Fake {
    fn execute(&self, call: &Call, _: &Authorization, _: Instant) -> Outcome {
        let fault = self.held.lock().unwrap().faults.pop_front();
        match fault {
            None => self.act(call),
            Some(Fault::Lost) => {
                self.act(call);
                Outcome::Unknown(Unknown::ContactLost)
            }
            Some(Fault::Down) => Outcome::Unknown(Unknown::ContactLost),
            Some(Fault::Refuse(why)) => Outcome::Rejected(Rejection::Provider(why.into())),
            Some(Fault::Twice) => {
                let id = &call.identity;
                let resource = |name: &str| {
                    Resource::new(
                        id.attempt.clone(),
                        id.configuration.clone(),
                        ResourceId::new(name.into()),
                        "g1".into(),
                    )
                };
                let matches = ["twin-a", "twin-b"]
                    .map(|name| Receipt::new(id.clone(), resource(name), "seen".into()))
                    .to_vec();
                Outcome::Conflicting { matches }
            }
            Some(Fault::CrashBefore) => panic!("the worker died before it reached the provider"),
            Some(Fault::CrashAfter) => {
                self.act(call);
                panic!("the worker died before it heard the provider")
            }
        }
    }
}

/// The fake, but for the first call of `on`, after which `meanwhile` runs before its answer is
/// returned: what another worker or a caller does while that call is in flight.
pub struct Meanwhile<'a> {
    fake: &'a Fake,
    on: Operation,
    meanwhile: Mutex<Option<Box<dyn FnOnce() + Send + 'a>>>,
}

impl<'a> Meanwhile<'a> {
    pub fn new(fake: &'a Fake, on: Operation, meanwhile: impl FnOnce() + Send + 'a) -> Self {
        Meanwhile {
            fake,
            on,
            meanwhile: Mutex::new(Some(Box::new(meanwhile))),
        }
    }
}

impl Adapter for Meanwhile<'_> {
    fn execute(&self, call: &Call, authorization: &Authorization, deadline: Instant) -> Outcome {
        let outcome = self.fake.execute(call, authorization, deadline);
        if call.request.operation() == self.on {
            let meanwhile = self.meanwhile.lock().unwrap().take();
            if let Some(meanwhile) = meanwhile {
                meanwhile();
            }
        }
        outcome
    }
}
