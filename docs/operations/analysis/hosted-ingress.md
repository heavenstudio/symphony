# Hosted ingress and board readiness

Symphony can serve a hosted browser through an authenticated app proxy without extending the
local machine's trust to every visitor. The proposed boundary is an explicit hosted server
adapter: it verifies a visitor, resolves the project and authorizes the operation before any
handler reads a body or opens project state. Local starts retain their existing gate.

This is a design proposal, not a released hosting capability or deployment acceptance. The
direction is [the approved explicit adapter](#hosted-ingress-boundary). The
[server guide](../../../apps/symphony-server/README.md#hosted-admission) describes the additive
admission seam and its limits; the deployment requirements below still need consumer proof.
No live IAM, database,
server or consumer checkout is changed by it.

## What exists and what is missing

Inspection base: `8c30aa24fdbaa0b3ebfa97204197748f34885b32`. These relative links identify the
inspected modules; the proposal below is additional behavior, not a description of their API.

| Boundary | Inspected behavior |
| --- | --- |
| [Gate](../../../apps/symphony-server/src/gate.rs), [front](../../../apps/symphony-server/src/front.rs) | `start` and `Shared::beside` construct `Gate::tailscale()`. The request judge admits loopback, otherwise a verified tailnet peer. Host checks name loopback or this machine's tailnet. Neither check authenticates a hosted person. |
| [Reads](../../../apps/symphony-server/src/api.rs), [writes](../../../apps/symphony-server/src/write.rs), [feed upgrade](../../../apps/symphony-server/src/api/feed_socket.rs) | Native routes check Host; writes validate Origin before body consumption. Browser writes and WebSocket upgrades compare an `http://` origin with Host. Absence of Origin is permitted for local nonbrowser clients. These are local protections, not an HTTPS proxy policy. |
| [CLI](../../../apps/symphony-server/src/cli.rs), [front](../../../apps/symphony-server/src/front.rs) | `serve --root ROOT --host ADDRESS --port PORT` runs in the foreground and watches SIGTERM, SIGINT and SIGHUP. Private starts also watch their starter. Changing a bind address adds no authentication. |
| [Front status](../../../apps/symphony-server/src/front.rs), [health read](../../../apps/symphony-server/src/api.rs) | `/api/front` includes paths/process information and `ready` means a hub exists. `/api/health` reports hub health from memory. Neither is the proposed nonsecret hosted board-readiness contract. Startup can log a warmup error and continue. |
| [Client selection](../../../apps/symphony-server/src/client.rs) | A build may embed its client or select the matching installed distribution. A hosted image must select and verify those release bytes explicitly, without a developer preview folder. |

The [execution proposal](../../engineering/analysis/remote-execution-attempts.md#source-boundary) also identifies a gap:
current route/action plugin calls carry no authenticated human. A payload saying `owner`, an
agent handle or a runtime conversation ID cannot fill it.

## Two ways to put the proxy in front

| Choice | Smallest safe form | Cost and limit |
| --- | --- | --- |
| A: trusted loopback gateway | Gateway is the only public listener; core binds only a private loopback port. Gateway verifies identity, canonicalizes routes, authorizes every target, checks browser origin and proxies streams. It hides core diagnostics and machine controls. No other process can reach that private listener. | Keeps much core routing intact, but all admitted requests become machine-local to it. Rewriting Host/Origin to pass today's gate loses the original security context. Gateway must duplicate project/body routing and know every new route; private loopback alone cannot distinguish another process in the container. Core needs changes for actor propagation and useful readiness anyway. |
| B: explicit hosted adapter, recommended | One foreground server selects a hosted ingress policy at startup. Its trusted host verifies the proxy boundary and signed visitor; generic admission consumes typed identity, canonical external origin, selected project and grants. Route handlers share that authorization context. | Requires an additive ingress/actor seam, HTTPS-aware origin checks and hosted lifecycle/readiness. It avoids a second routing policy and the implicit loopback privilege. Vendor token/header and transport handling stay in the host adapter. |

Neither option is a switch that disables the gate. A is acceptable only with its complete
gateway policy and private-core isolation proved; merely forwarding to localhost and replacing
Host/Origin is insufficient. B is the smaller complete change once per-project authorization,
actor attribution and readiness are included. This is a code-boundary comparison; no latency,
binary-size or implementation-time measurement has been made.

The first hosted profile serves **one explicitly selected project root per app**. It uses no
machine registry or automatic discovery of other checkouts. This limits the target set; it does
not grant every app visitor access to that project. Multi-project hosting would extend the same
project resolver and grants, with filtered project lists/counts, rather than inheriting the
machine-wide registry.

- [r] **hosted-ingress-boundary** — A hosted Symphony needs to check who is visiting and which project they may use. Its current localhost access trusts the machine, so pointing an authenticated proxy at it does not establish those permissions. Where should the hosted checks live? A: a separate gateway that checks every request and forwards to a private local server. B: an explicit hosted adapter in the server that passes verified visitor and project permissions to its handlers. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/hosted-ingress#Two ways to put the proxy in front]]
  [2026-11-17 19:46] agent — A keeps most core routing, but must duplicate project/action checks and isolate the private listener; rewriting Host and Origin alone is unsafe. B adds an admission/actor seam and hosted lifecycle/readiness, with one routing policy. Both preserve local defaults. No runtime implementation, timing or deployment validation is claimed.

    **A condition of either choice.** The inspected platform allows some privileged callers to reach the raw service. Proxy-only ingress must be proved through effective transport policy or additional verified proxy attestation before hosted access is enabled. A valid visitor token alone does not establish that path. [[analysis/hosted-ingress#Direct-origin limitation to resolve before deployment]]

    **Earlier verdicts checked.** **desktop-client** chooses a thin Mac shell and explicitly leaves hosted HTTPS/login for a later card. **consumer-contract-versioning** preserves the public kit/helper boundary; either choice keeps vendor handling in the consuming host. **release-delivery** chooses distribution, not visitor authorization. **server-socket-hand-over** keeps the machine's shared-server handover; this proposal preserves that and specifies a separate foreground hosted lifecycle. None decides this ingress boundary. This card asks the architecture direction only; implementation, consumer deployment policy and live changes need their own scoped work.
  [2026-11-17 20:36] owner · approved — B
  [2026-11-17 20:59] agent — Your “B” selects an explicit hosted adapter in the server, passing verified visitor identity and project permissions to its handlers. Implementation will preserve the local mode and prove the hosted authorization boundary. This decision does not itself deploy or expose the server.
  [2026-11-17 20:59] agent — Recovered the exact callback reply supplied by its writer after a concurrent save omitted it; original owner verdict retained.
  [2026-11-17 20:59] agent · resolved

## Identity and the trusted proxy boundary

There are two separate credentials: the proxy's credential for the runtime ingress and the
visitor's signed assertion for the app. Success at one cannot substitute for the other.

The pinned [platform visitor contract][visitor] specifies RS256, an issuer whose keys are at its
`/auth/jwks`, a 15-minute identity, and audience `app:<immutable-app-id>`. Its
[mint and claims][mint] set `sub` to the person's email and optionally carry `org_id`.
The app route/name is reusable and is not the audience to accept. These claims identify a
visitor; they confer no Symphony project permission.

The consuming host adapter maps protected deployment configuration into an exact issuer, key
endpoint, immutable app audience, canonical external origin and proxy-boundary policy. Generic
Symphony code names no platform environment variables or visitor header. Configuration cannot
come from the visitor's token, request Host, query parameters or project document being edited.
The adapter must:

1. Read exactly one assertion from the configured platform header; reject duplicates,
   malformed/oversize tokens and unsigned identity substitutes. Pin RS256 and the configured
   issuer's signing keys; refuse `none`, algorithm confusion and untrusted key URLs in a token.
2. Verify signature and exact `iss` and string `aud` before using any claim. Refuse a route-form,
   preview, general-login or other-app audience, even if the signature is valid.
3. Require valid `exp` and `iat`, current time before expiry and a lifetime no greater than the
   contract's 15 minutes. Reject future issue times and invalid ordering, using an injected clock
   in tests. No expired token is accepted through a key-cache or clock-error fallback.
4. Fetch/cache keys only from the configured HTTPS endpoint with bounded requests, normal TLS
   verification and issuer/key separation. Unknown keys may trigger one bounded refresh;
   unavailable or expired key material cannot turn verification into decoding without checking.
   Key refresh failure yields an unavailable response when verification cannot be established.
5. Produce an internal principal from verified issuer, subject and tenant. This provider's
   subject is an email, not a promised immutable user UUID. Grant provisioning must account for
   reassignment; never authorize from display email, domain suffix or an unsigned header. A
   tenant-scoped grant requires a matching verified tenant; missing tenant does not default to
   the deployment's tenant.

The host attests the **transport boundary independently of visitor identity**. Only a configured,
verified proxy hop may supply authoritative forwarded scheme, host, port or address. Trust
neither arbitrary `Forwarded`/`X-Forwarded-*` chains nor a source IP merely belonging to a cloud
range. The proxy must strip client-supplied identity/forwarding headers and set its own single
authoritative values. An unverified request cannot enable hosted admission by setting them.

### Direct-origin limitation to resolve before deployment

The pinned platform [serving contract][serving] uses a Google ID token for proxy-to-runtime
authentication. Its [direct-origin section][direct] also accepts inherited privileged roles
that can bypass the proxy. A per-service proxy-only invoker binding therefore does not prove
exclusive ingress. A signed visitor assertion is a bearer credential; its valid app audience
alone does not prove that the current HTTP request traversed the proxy.

For this proposal, public/raw-origin calls must be refused without the verified proxy boundary,
even if they forge forwarded headers or carry a copied visitor assertion. Deployment acceptance
must demonstrate either an exclusive authenticated network/IAM path, including effective
inherited invoke grants, or additional cryptographic proxy attestation bound to this request
and verified by the host. That attestation would need a defined method/path/body-digest binding,
expiry and replay policy; the inspected contract does not supply one. A header shared secret or
an external-Host comparison alone is not that proof.

The consumer/platform owner must choose and prove the available transport mechanism. Until
then hosted admission is unavailable, not an automatic loopback fallback. Administrators able
to redeploy the verifier or change its trust configuration remain in the deployment's trusted
computing base; this design cannot protect the service against its own administrator. No live
IAM or direct-origin test was performed here. [Cloud Run ingress controls][ingress] describe
available controls, not the effective policy of any deployment.

## Project admission and actor propagation

After transport and identity verification, one generic admission step resolves a canonical
project and operation and evaluates explicit grants. Default is deny. For the first profile a
grant binds the verified principal to the deployment's configured project ID and permissions
such as read, edit and a separately enumerated control action. An app-level access check is not
that grant. A read grant never becomes edit, owner, reviewer, staffer or landing authority.

| Surface | Admission requirement |
| --- | --- |
| Shell, retained client builds, project lists/counts, documents, history/diff, board reads and plugin reads | Authenticate before serving; authorize the resolved project and read capability. Filter aggregate results or omit them. A cache hit/ETag/HEAD takes the same checks as a full read. |
| Saves, comments, uploads, task/actions and plugin submissions | Authorize the route/project/action before reading a body. Decode within size limits without opening target state; if the body names another scope, resolve and authorize that target before dispatch or project I/O. Keep existing path containment, metadata exclusion, entrypoint and MIME checks. |
| Media, attachment metadata, thumbnails, raw files, logs and every byte/range read | Pass server-derived principal and current tenant/project membership to metadata operations and every byte request, including HEAD, range, conditional and cache reads. A digest, artifact ID, local path or storage locator is not permission. Authorize before proxying private objects through the artifact provider. |
| SSE and WebSocket upgrades | Authorize the project's feed and validate browser Origin before opening a stream. Each reconnect authenticates and authorizes again. A cursor or subscription cannot choose a different project. |
| Machine/front diagnostics, stop/reload, local pane/editor/focus and filesystem controls | Excluded from the hosted route set. No request becomes `is_this_machine` merely because an adapter runs locally. Future remote controls need their own action capability and audit. |

Use the same resolver for encoded/normalized paths, project prefixes, branch scopes, body
targets and storage references, before any project lookup can expose another project's data.
Ambiguous encodings, conflicting authorities and traversal fail. An unknown or unauthorized
project returns the same nonrevealing miss; authentication failures return 401, denied actions
403, unavailable verification/dependencies 503. No refusal echoes tokens, paths, DSNs or claims.

The admitted context carries principal, project, allowed action and authorization-policy revision
to save/plugin/provider dispatch and audit; preserve initiating human separately from acting
service/attempt. Payload actor fields never override it. Extend public plugin APIs additively
under the [two-release rule](../../workflow/CHECKS.md#consumer-api-changes). A legacy plugin lacking actor
context can serve only an explicitly audited capability that does not need it; it cannot gain
hosted mutation privileges by silently discarding the context.

Grants must be rechecked on every operation. Existing streams end at assertion expiry and on a
local policy-revocation event; external revocation must reach the adapter or have an explicitly
documented bounded refresh contract. The platform's route cache and token lifetime do not
provide immediate revocation. During loss of the policy authority, no new grant is inferred;
the configured validity bound ends existing authorization. No credentials or visitor assertions
are put in URLs, browser storage, board events, logs or downstream artifact references.

Hosted artifact responses use `Cache-Control: private, no-store`, with shared proxy caching
disabled. The local upload handler's public immutable cache policy is not a hosted access
contract. Revocation denies later byte reads, cache hits and 304 responses; active downloads
are bounded by the authorization lifetime and cancelled on a revocation event. Already
delivered bytes cannot be recalled. Storage receives admitted identity and authority, never a
visitor token or a client-supplied membership assertion.

## Browser behavior

The host configures exact external HTTPS authorities and origins, including explicit ports
where applicable. No wildcard suffix matching, reflection from Host or discovery from forwarded
headers. The first profile uses one canonical origin and base `/`; aliases redirect only after
authentication to a configured canonical origin. Parse Host/authority once, reject duplicates,
userinfo, invalid ports and disagreement with trusted proxy metadata. Check it for assets and
upgrades as well as API calls.

All browser mutations require an exact allowed Origin, same-origin fetch metadata when present,
the expected content type and an unguessable CSRF token bound to the authenticated session and
project. Missing, `null`, malformed or foreign Origin is refused in hosted browser mode. Use a
host-only Secure cookie for any app session/CSRF binding and keep it separate from proxy login
cookies. A dedicated nonbrowser API credential may omit Origin only through an explicit
capability; a missing Origin is not proof of a nonbrowser client. Existing local exceptions stay
local. CORS grants no ambient cross-origin credentialed access.

Asset URLs and redirects derive from the configured external base, never an unverified forwarded
header. Serve the bundled client and retained builds on that origin. Authorize HTML/raw-file
delivery before its sandbox rules apply. Keep redirects relative or restricted to configured
destinations; proxy login stays proxy-owned. Do not redirect a body-bearing API request to a
sign-in page or forward identity to a storage redirect. Auth failures for scripts, images,
fetches and upgrades must remain status failures, not a successful HTML login body. The platform
[cookie/redirect boundary][cookies] is additional protection, not an app-side CSRF substitute.

SSE must flush without proxy buffering and disable shared caching. WebSockets must preserve
Upgrade semantics. No stream stays authorized past assertion expiry; reconnect gets a fresh
proxy assertion and grant. Close cleanly on drain, policy loss or expiry. The client reuses its
last project feed cursor only for that project; when a restart loses replay history, send a
reset/full snapshot instead of accepting a silently incomplete cursor. Use the existing client
retry helpers, backoff and deduplication; authentication failures wait for renewed sign-in rather
than an endless retry loop. [Cloud Run streams][streams] remain bounded requests and can
reconnect to another instance, so neither sticky sessions nor a process-local cursor is a
durability contract.

## Foreground Linux lifecycle

The deployment packages a Linux server plus its matching client and selected project inputs.
The existing foreground invocation provides the process/bind shape below; **it does not yet
enable the proposed hosted adapter**. With no approved adapter installed this command retains
the local gate and is not a hosted deployment recipe:

```sh
exec /opt/symphony/bin/symphony-server serve \
  --root /srv/project --host 0.0.0.0 --port "${PORT:?PORT is required}"
```

The future hosted host must validate assigned `PORT` as a nonzero valid port and explicitly
select the hosted configuration before binding. The runtime [container contract][runtime]
requires its ingress listener on `0.0.0.0` and the assigned port. An exec-form entrypoint, or a
shell ending in `exec`, makes the server PID 1. If a supervisor is necessary it owns forwarding
signals and reaping every child; no daemonization, terminal ownership or unowned worker.

The hosted profile has no Tailscale lookup, developer-machine registry, source rebuild/reload,
local launcher or installed-machine `current` dependency. The deployment selects one canonical
root and pins its project ID and input Git revision. On fresh disk, restart or scale-zero,
restore exact retained Git configuration/document revisions and artifact object generations
whose checksum and exact size the storage contract verifies before availability. Missing or
corrupt required revisions/bytes make restore incomplete, never a fallback to latest or pod-only
files. Backup provenance cannot reinstate access: evaluate current grants, and have the
storage/attempt contracts invalidate pre-restore execution/upload authority before enabling
writes. Do not discover a different root from the process's cwd or treat ephemeral disk as
durable state. Retained edits and restart behavior depend on the
storage work, not on ingress alone. Do not enable concurrent writers until that storage contract
proves them safe across old/new revisions; max-instances one alone does not prevent overlap.

SIGTERM first changes readiness to false and stops new mutation admission. Finish an admitted
save only within the shutdown deadline and its storage transaction/atomic-write contract; never
return success before its required durable publication. End feeds, cancel unadmitted body reads,
join workers, release connections and reap children. Cloud Run's documented SIGTERM-to-SIGKILL
window is ten seconds: the local replacement drain of 120 seconds is not a hosted shutdown
budget. Forced termination can leave an ambiguous response, so reconnect reads authoritative
state; it never blindly retries a write with a possibly committed effect. Tests must exercise
SIGTERM during body read, persistence and response delivery. Restart/migration recovery belongs
to the database/storage contracts.

## Readiness means a usable board

Proposed probe routes `/health/live` and `/health/ready` are exact, read-only exceptions to visitor
authentication for the runtime's probes. They expose no project lookup, mutation or arbitrary
path fallback. A probe bypass grants no access to `/api/front`, assets or any project route.
Liveness says the process can answer; database failure does not by itself fail liveness.

Readiness is 200 only when all required components of the selected project are usable:

- ingress/issuer/audience/origin configuration is valid and trusted-boundary verification and
  key/policy material are usable;
- the exact project root/revision is materialized, and the matching shell and referenced client
  assets are present and readable;
- the selected database, login, project identity and recovery generation are verified; its
  engine/schema combination is proved compatible with this writer, and a real bounded project
  board read through the shared connection pool has completed;
- required project-state restore is complete with verified revisions/generations; provider
  initialization alone is insufficient, and no stop, migration or writer fence makes the service
  unavailable.

Probe bodies contain only state, nonsecret build revision, client revision and coarse component
states such as `board_unavailable`, `schema_incompatible` or `draining`; use `Cache-Control:
no-store`. Keep paths, subject/tenant, connection strings, credentials and raw errors in neither
body nor headers. Revision identifiers are deployment provenance, not filesystem paths.
Prefer bounded, recently successful component observations with explicit freshness/expiry;
do not run a migration or expensive full scan on every probe. Probe reads and connections count
within the shared pool and deployment connection budget, with no extra probe-only connection
outside the cap. No available pool capacity means 503. A stale success cannot survive the
freshness bound or a known dependency failure. Reconnect revalidates database/login/project,
recovery generation and engine/schema before a successful board read restores readiness.
An unavailable or incomplete storage result keeps required restore at 503; an optional artifact
failure denies its affected delivery explicitly and does not imply that every board operation
failed. Neither a provider nor a worker being up proves durable state was restored, and byte
availability alone grants no task success, phase advancement, stamp or landing.

Database outage, exhausted capacity, unknown/newer schema, migration in progress or failed first
board read yields 503 readiness and 503 dependent operations, without a local fallback or stale
successful mutation. The database contract's proved-compatible, one-version-behind read-only
access may support authorized diagnostics; it cannot make a normal writing application ready
when its writer is incompatible. That deployment keeps readiness at 503. An intentionally
configured document-only service could have a different
contract; it cannot call itself board-ready. An open TCP port or a rendered shell proves only
that those bytes were served. The consumer must configure the HTTP startup and readiness probes
to use this readiness response under the [runtime health-check contract][probes], with liveness
separate. No probe redirects to login: the platform counts redirects as probe success. A
one-time startup success alone does not handle a later DB outage.

## Delivery boundaries and validation

The [managed database task](../../workflow/TASKS.md) `tools/hosted-postgresql` owns external connection,
version/privilege validation, migrations, capacity and stale-writer fencing. The separate
`tools/remote-artifact-store` owns opaque artifact references, durable publication, Git/config
rehydration and recovery. Link their landed designs when available; do not import another live
branch's files. The [attempt proposal](../../engineering/analysis/remote-execution-attempts.md#plugins-and-trusted-actors)
owns attempt/runtime identity and fencing. Ingress passes verified actor/project/action to those
boundaries, and readiness consumes their actual availability. A healthy ingress never certifies
a dispatcher, remote job, artifact restore or completed task.

Before implementation, split additive admission/actor APIs, the host verifier, lifecycle/probes
and client-origin/reconnect work into focused tasks. Keep local behavior, compile guards and
legacy public APIs working. Platform SDKs, credentials, resource provisioning and deployment
authorization policy remain in the consuming adapter. This design does not authorize them.

| Future isolated validation | Required observation |
| --- | --- |
| Local regression | Loopback, verified tailnet, rejected peer and rebinding Host behavior unchanged, without hosted configuration. Hosted mode works with no Tailscale executable/state. |
| Visitor verification | Valid signature/issuer/app/tenant succeeds only with a project grant. Missing/duplicate/forged/expired/future/unknown-key/other-issuer/route/other-app assertions fail before body/state access. Inject clock and key rotation/outage; verify no fail-open cache. |
| Proxy and browser boundary | Direct origin, forged forwarded headers, extra chains, invalid/conflicting Host and foreign/null/missing Origin fail. Test CSRF omission/mismatch and encoded target aliases across reads, saves, media/ranges, assets and upgrades. |
| Project isolation | Same subject with another tenant, no grant, read-only grant, another project in path/body/artifact/cursor and a legacy plugin cannot read or mutate outside admission. Check current membership on metadata and every byte read, including HEAD/range/conditional/cache; revocation denies 304/cache hits and cancels active downloads. |
| Startup and probes | Assigned/missing/zero/invalid PORT; wrong root/revision; missing/mismatched client; wrong database/login/project or recovery generation; bounded board read blocked; shared pool exhausted with no probe connection outside its cap; unsupported engine/schema and migration. Compatible read-only diagnostics cannot ready an incompatible writer. Reconnect revalidates identity/schema; known outage invalidates prior success. TCP/shell success never makes readiness 200. No diagnostics secrets escape. |
| Shutdown and restart | PID 1 receives SIGTERM during save/body/commit/response; readiness drops, new writes fail, committed state survives restart and incomplete state is explicit. Children are reaped; no duplicate mutation after an ambiguous response. Empty disk uses exact retained Git revisions and verified object generations; missing/corrupt required state stays unready, no latest fallback. Restored grants and old execution/upload authority cannot regain access. |
| Feeds and revocation | SSE/WebSocket across timeout, deployment/restart and another instance; cursor gap/reset, refreshed identity, token expiry, policy change and auth loss. No cross-project events or unauthorized continued stream. |

Unit tests use injected clocks, keys, policy and provider fakes, including refusal cases; focused
Linux HTTP/process tests use a temporary machine folder, private port and scratch board. Analyze
coverage of admission, normalization, claims and readiness transitions after implementation;
run the closest regressions, lint and independent review required by [checks](../../workflow/CHECKS.md).
No shared-service restart and no task-level full verification. No implementation tests, cloud
probe, IAM acceptance, cold-start or shutdown measurement were run for this document task.

## Sources

The external platform sources below are pinned to
`1f90ba603d1954dcb77c700a63503024fb544b37` and were read through the authenticated GitHub API.
They establish an interface to design against, not a deployment's effective settings. Google
documentation describes platform constraints and must be rechecked when an implementation is
accepted. This proposal does not use IAP's signed-header format: that is a different issuer,
algorithm and audience contract from the inspected app proxy.

[visitor]: https://github.com/toptal/maestro/blob/1f90ba603d1954dcb77c700a63503024fb544b37/docs/guides/app-hosting-architecture.md#visitor-identity
[mint]: https://github.com/toptal/maestro/blob/1f90ba603d1954dcb77c700a63503024fb544b37/maestro-identity/src/jwt.rs
[serving]: https://github.com/toptal/maestro/blob/1f90ba603d1954dcb77c700a63503024fb544b37/docs/guides/app-hosting-architecture.md#serving-a-request
[direct]: https://github.com/toptal/maestro/blob/1f90ba603d1954dcb77c700a63503024fb544b37/docs/guides/app-hosting-architecture.md#accepted-raw-runapp-access-for-a-maestro-gcp-identity
[cookies]: https://github.com/toptal/maestro/blob/1f90ba603d1954dcb77c700a63503024fb544b37/docs/guides/app-hosting-architecture.md#the-trust-boundary-and-why-cookies-are-filtered
[runtime]: https://docs.cloud.google.com/run/docs/container-contract
[streams]: https://docs.cloud.google.com/run/docs/triggering/websockets
[ingress]: https://docs.cloud.google.com/run/docs/securing/ingress
[probes]: https://docs.cloud.google.com/run/docs/configuring/healthchecks
