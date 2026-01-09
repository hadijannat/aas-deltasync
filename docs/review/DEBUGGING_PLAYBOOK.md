# Debugging and Verification Playbook (Expert Review)

## 1. Purpose and review goals

This playbook is for expert reviewers evaluating the repo's core claim:

> The agent provides offline-first, multi-master synchronization for AAS-based industrial digital twins, using delta-CRDT anti-entropy replication, supporting BaSyx MQTT event ingestion (primary) and REST polling fallback, and ensuring deterministic convergence after network partitions and concurrent writes.

### What "functional as claimed" means in this repo

A review is "passed" if you can demonstrate all of the following:

1. **AAS Part 2 HTTP/REST correctness**

   * Identifiers of Identifiables used in path and query parameters are **base64url encoded** (no padding).
   * `idShortPath` is **URL encoded** (current encoder preserves square brackets in paths like `[index]`). ([industrialdigitaltwin.io][1])

2. **BaSyx MQTT event ingestion correctness**

   * The agent correctly subscribes to BaSyx Submodel Repository topics for element updates and patches and produces internal update events. ([wiki.basyx.org][2])
   * If BaSyx is configured for empty value update events, the agent detects missing values and fetches them via REST to avoid state loss. ([wiki.basyx.org][3])

3. **Delta-CRDT replication correctness**

   * The agent disseminates deltas and uses a join/merge operation to converge state.
   * The replication tolerates at-least-once delivery and duplicate messages without divergence (idempotence). ([arXiv][4])

4. **Partition tolerance and convergence**

   * With an induced network partition (MQTT disrupted), both sides accept writes independently, and after healing, converge deterministically to the same logical state. ([arXiv][4])

5. **FA3ST HTTPS compliance (if FA3ST adapter is enabled)**

   * The adapter supports HTTPS, consistent with FA3ST's documented requirement that only HTTPS is supported since AAS v3.0. ([faaast-service.readthedocs.io][5])

---

## 2. Pre-flight: toolchain and diagnostic tools

You will need:

* Docker + Docker Compose
* `curl`
* `jq`
* MQTT client tools: `mosquitto_sub`, `mosquitto_pub`
* A network fault injector:

  * Recommended: Toxiproxy (deterministic) ([GitHub][6])
* If using Rust implementation: `cargo`, `rustfmt`, `clippy`

Optional but highly recommended for review rigor:

* Wireshark or tcpdump (MQTT traffic inspection)
* Prometheus/Grafana if the agent exports metrics

---

## 3. Repository orientation: where the "truth" lives

Reviewers should start by locating and skimming these items:

### 3.1 Specs and API correctness anchors

* `crates/aas-deltasync-adapter-aas/src/encoding.rs` (canonical base64url + idShortPath encoding)
* `crates/aas-deltasync-adapter-aas/src/client.rs` (HTTP path builder that should always use the encoders)

Your implementation must embody the AAS API rule:

* Identifiers are base64url encoded in the API
* `idShortPath` is URL encoded ([industrialdigitaltwin.io][1])

Note: `specs/` currently contains placeholders; use the external IDTA HTTP/REST spec as the normative reference.

Also check the AAS FAQ because reviewers often trip over `Submodel Id` vs `idShort` usage:

* In current API versions, a base64url-encoded Submodel Id is expected for `GET /submodels/{submodelIdentifier}` (not `idShort`). ([GitHub][7])

### 3.2 BaSyx event ingestion anchors

* `crates/aas-deltasync-adapter-basyx/src/events.rs` (topic parsing + payload shape)
* `crates/aas-deltasync-adapter-basyx/src/subscriber.rs` (subscription + QoS)
* Must implement topic parsing per BaSyx Submodel Repository MQTT eventing docs. ([wiki.basyx.org][2])

### 3.3 CRDT and delta replication anchors

* `crates/aas-deltasync-core/src/crdt.rs` (LWW register, OR-Map, delta types)
* `crates/aas-deltasync-core/src/document.rs` (document-level delta generation + apply)
* `crates/aas-deltasync-core/src/merge.rs` (AAS element merge strategy)
* `docs/design/crdt-mapping.md` (mapping assumptions)
* `crates/aas-deltasync-proto/src/messages.rs` (DocDelta + anti-entropy message types)
* `crates/aas-deltasync-proto/src/topics.rs` (topic scheme for delta + AE request/response)
* `crates/aas-deltasync-agent/src/replication.rs` (delta publish/subscribe)
* `crates/aas-deltasync-agent/src/persistence.rs` (delta log + peer progress)
* `crates/aas-deltasync-agent/src/runtime.rs` (replication loop; incoming delta apply is TODO)

Delta-CRDT design should be traceable to the delta-state CRDT framework and its anti-entropy mechanisms. ([arXiv][4])

If using the Rust `crdts` crate, reviewers should verify correct usage:

* `map` module for observed-remove maps
* `orswot` for sets
* `lwwreg` (with careful monotonic marker selection) ([Docs.rs][8])

### 3.4 Clocking anchors (to avoid LWW divergence)

If you use LWW registers, reviewers should check how you ensure monotonic causal markers.

The `crdts` LWW register documentation explicitly warns: the causal marker must be monotonic and globally unique, and "don't use timestamps unless you are comfortable with divergence." ([Docs.rs][9])

Therefore the codebase should use a safer mechanism, commonly:

* Hybrid Logical Clock (HLC) or similar monotonic logical timestamping ([cse.buffalo.edu][10])

Reviewers should locate:

* `crates/aas-deltasync-core/src/hlc.rs`
* Tests for monotonicity under clock skew simulation (in the same module)

---

## 4. Code review checklist: "read the code and prove the claims"

This section is the code-reading path for an expert, in the order that yields the fastest confidence.

### 4.1 Verify AAS identifier encoding is correct everywhere

Search for:

* `base64url`
* `idShortPath`
* `submodelIdentifier`
* `aasIdentifier`

The implementation must ensure:

* All Identifiable IDs passed in HTTP paths or query parameters are base64url encoded, no padding.
* All `idShortPath` segments are URL encoded; current encoder preserves brackets. ([industrialdigitaltwin.io][1])

Typical review technique:

* Ensure there is a single canonical encoding function used everywhere.
* Ensure it is unit tested with golden values.
* Ensure adapters do not "double encode" or accidentally treat base64url as standard base64.

Red flags:

* Hand-built URL strings without a centralized encoding function.
* Confusion between "base64" and "base64url".
* Path concatenation that does not encode `[` and `]`.

### 4.2 Verify BaSyx MQTT topic parsing matches documented topics

Per BaSyx Submodel Repository MQTT eventing docs, reviewers must confirm handling at minimum:

* `.../submodelElements/<idShortPath>/updated`
* `.../submodelElements/<idShortPath>/deleted`
* `.../submodelElements/patched` ([wiki.basyx.org][2])

Also verify handling for "empty value updates":

* When a qualifier configures empty value updates, payloads omit the value and the agent must fetch it. ([wiki.basyx.org][3])
* In this repo, `crates/aas-deltasync-adapter-basyx/src/events.rs` surfaces `value: Option<...>`; reviewers should confirm the ingestion layer fetches missing values when `None`.

Red flags:

* Topic parsing that assumes `idShortPath` contains no slashes or brackets.
* Ignoring patched events and only handling single-element updates.
* Assuming payload always includes value.

### 4.3 Verify your CRDT mapping is deterministic and mergeable

Reviewers should check these invariants:

* Every "AAS element path" maps to exactly one CRDT key, deterministically.
* All concurrent updates are resolved deterministically (no order dependence).
* Deletions are observed-remove semantics (tombstone or dot-based remove), not naive delete.

If you use `crdts::Map`, `crdts::orswot`, and `crdts::LWWReg`, confirm you are using them with monotonic markers and correct contexts. ([Docs.rs][8])

Red flags:

* Using wall clock milliseconds directly as a monotonic marker without a monotonicity strategy.
* Merge logic that depends on message arrival order.

### 4.4 Verify delta generation and anti-entropy logic matches delta-CRDT principles

Delta-CRDT expectations:

* A mutation produces a delta-state that is joined into the local state.
* Deltas are disseminated over unreliable channels.
* Anti-entropy ensures eventual convergence. ([arXiv][4])

Reviewers should locate:

* Delta mutators and join: `crates/aas-deltasync-core/src/crdt.rs` and `crates/aas-deltasync-core/src/document.rs`.
* Anti-entropy message types + topics: `crates/aas-deltasync-proto/src/messages.rs` and `crates/aas-deltasync-proto/src/topics.rs`.
* De-duplication and peer tracking: `crates/aas-deltasync-agent/src/persistence.rs` (delta log + peer progress).
* Runtime wiring: `crates/aas-deltasync-agent/src/runtime.rs` (incoming delta apply is currently TODO).

Red flags:

* "Delta" messages that are actually full-state snapshots each time.
* Lack of idempotence tests.
* Lack of peer progress tracking (no way to know what a peer has seen).

### 4.5 Verify FA3ST adapter enforces HTTPS capability

If FA3ST adapter exists, reviewers must confirm:

* TLS is supported and defaulted on (`crates/aas-deltasync-adapter-faaast/src/poller.rs`).
* Certificate verification behavior is explicit (see poller config).
* The adapter docs note HTTPS-only (`crates/aas-deltasync-adapter-faaast/src/lib.rs`) and align with FA3ST's HTTPS requirement since AAS v3.0. ([faaast-service.readthedocs.io][5])

---

## 5. Runtime verification: demonstrate the claims end-to-end

This section is designed so reviewers can "see it work" and collect artifacts that prove functionality.

### 5.1 Bring up the demo topology

Use the repo-provided compose:

```bash
docker compose -f examples/docker-compose.yml up -d
docker compose -f examples/docker-compose.yml ps
```

Expected running components:

* MQTT broker (Mosquitto or equivalent)
* Site A AAS server (BaSyx)
* Site B AAS server (BaSyx)
* Agent A / Agent B (commented out by default in `examples/docker-compose.yml`)

### 5.2 Verify BaSyx MQTT event emission directly (independent of the agent)

This is the fastest "ground truth" for the event feed.

Subscribe:

```bash
mosquitto_sub -h localhost -p 1883 -t 'sm-repository/#' -v
```

Now modify a SubmodelElement on the BaSyx side (via your provided script or a direct REST call). You should see topics consistent with the documented patterns, for example:

* `sm-repository/<repoId>/submodels/<submodelIdBase64URLEncoded>/submodelElements/<idShortPath>/updated` ([wiki.basyx.org][2])

If you do not see events:

* Check BaSyx MQTT configuration.
* Check broker connectivity.
* Confirm the repository has MQTT feature enabled (BaSyx component docs cover MQTT eventing features). ([wiki.basyx.org][2])

### 5.3 Verify the agent consumes BaSyx events and turns them into deltas

Run agent logs at debug/trace:

```bash
docker logs -f agent-a
docker logs -f agent-b
```

If you run the agent locally instead (agents are commented out in the compose file), use:

```bash
RUST_LOG=debug cargo run -p aas-deltasync-agent
```

Expected log signals (current code + added trace points):

* Agent startup: "Starting AAS-ΔSync Agent", "Agent initialized", "Starting agent runtime"
* BaSyx subscriber: "Subscribing to BaSyx events", "Connected to MQTT broker", "Subscription acknowledged"
* Event parsing: "Parsed BaSyx event" (repo id, submodel id, idShortPath, has_value)
* Replication: "Subscribing to replication topic", "Publishing delta"
* Incoming replication: "Received replication message"
* CRDT changes: "Created delta (set)", "Created delta (remove)", "Applied delta"
* Shutdown: "Shutdown signal received", "Agent stopped"

Note: runtime wiring for applying incoming deltas is still TODO in `crates/aas-deltasync-agent/src/runtime.rs`, so the "Applied delta" log appears once deltas are actually applied.

### 5.4 Validate AAS REST API encoding correctness with one concrete call

Pick an actual Submodel identifier (IRI or UUID) in your demo and base64url encode it.

A quick encoding check using Python:

```bash
python3 - << 'PY'
import base64
s = "urn:example:submodel:1"
print(base64.urlsafe_b64encode(s.encode()).decode().rstrip("="))
PY
```

Then call the AAS API using the base64url id:

```bash
curl -sS "http://localhost:<sm_repo_port>/submodels/<BASE64URL_SUBMODEL_ID>/$value" | jq .
```

The reason this matters is that AAS Part 2 requires identifiers in the API to be base64url encoded, and `idShortPath` must be URL encoded. ([industrialdigitaltwin.io][1])

Also note that current API specs expect unique base64url-encoded Submodel Id for `/submodels/{submodelIdentifier}` rather than idShort. ([GitHub][7])

### 5.5 Validate cross-site replication without partitions

Procedure:

1. On Site A, set `Property X = 10`.
2. Confirm:

   * BaSyx emits MQTT update event (independent check in 5.2).
   * Agent A publishes a delta.
   * Agent B receives and applies it.
3. On Site B, query AAS REST `$value` view and confirm `X=10`.

Tip for reviewers:

* Always compare `$value` view, not "normal" view, because `$value` isolates the frequently changing state and avoids noisy metadata. The IDTA docs list `$value` as one of the standard parameter paths in the OpenAPI file. ([industrialdigitaltwin.io][11])

### 5.6 Prove partition tolerance with deterministic fault injection

Use Toxiproxy to disrupt MQTT connectivity for Agent B while leaving both AAS servers reachable.

Toxiproxy is designed to simulate network conditions for testing and CI and supports deterministic tampering. ([GitHub][6])

High-level steps:

1. Run Toxiproxy in the compose and route Agent B's broker connection through it.
2. "Cut" the connection (timeout or disable).
3. Perform concurrent writes:

   * Site A sets `X=10`
   * Site B sets `X=20`
4. Restore connectivity.
5. Verify both replicas converge to the same value and the same state digest.

Artifacts reviewers should collect:

* MQTT transcript (subscribe to agent delta topics and record).
* Final `$value` JSON from Site A and Site B.
* Agent logs showing merge.

### 5.7 Prove idempotence and duplicate delivery safety

This is mandatory for any at-least-once MQTT setup (QoS 1).

Procedure:

* Capture a `DocDelta` payload (or your wire equivalent).
* Re-publish it verbatim to the same topic.
* Confirm state digest does not change after the second application.

This validates delta de-duplication keyed by `delta_id` or equivalent.

### 5.8 Prove restart safety (persistence)

Procedure:

1. Let the system converge.
2. Stop Agent B container.
3. Make several changes on Site A.
4. Restart Agent B.
5. Confirm Agent B catches up via anti-entropy (or retained deltas) and converges.

Reviewers should confirm the persistence layer stores enough information to resume:

* Local state snapshot
* Delta log and/or peer progress

---

## 6. Debugging decision tree: diagnose by symptom

### Symptom A: "REST calls fail or return 404 for known objects"

Most common root cause:

* Mis-encoding of identifiers or idShortPath.

Actions:

1. Check that Identifiable ids in path are base64url encoded, no padding.
2. Check that idShortPath is URL encoded; current encoder preserves brackets.
3. Confirm you are using Submodel Id, not idShort, in `/submodels/{submodelIdentifier}` calls. ([industrialdigitaltwin.io][1])

### Symptom B: "No BaSyx events are seen"

Actions:

1. Subscribe directly to `sm-repository/#` and confirm the repository emits events (5.2).
2. Confirm the configured MQTT broker address is reachable from the repository container.
3. Confirm MQTT feature is enabled for the repository. ([wiki.basyx.org][2])

### Symptom C: "Events are seen but the agent does not apply updates"

Actions:

1. Check topic parsing. BaSyx includes base64url submodel id and URL-encoded idShortPath in topic segments. ([wiki.basyx.org][2])
2. Check payload parsing compatibility.
3. Check empty value update handling (payload may omit value). ([wiki.basyx.org][3])

### Symptom D: "Divergence after partitions or under concurrent writes"

Actions:

1. Verify LWW markers are monotonic and globally unique.
2. If using `crdts::LWWReg`, confirm you did not use raw wall-clock timestamps without a monotonic strategy. The docs explicitly warn about divergence if timestamps are used naively. ([Docs.rs][9])
3. Confirm clock implementation is HLC or equivalent and has tests for skew. ([cse.buffalo.edu][10])
4. Confirm delta join is commutative, associative, idempotent and tested.

### Symptom E: "Agent cannot talk to FA3ST endpoint"

Actions:

1. Confirm you are using HTTPS. FA3ST states only HTTPS is supported since AAS v3.0. ([faaast-service.readthedocs.io][5])
2. Confirm CA / cert verification settings.
3. Confirm base path prefix and API version config.

---

## 7. Minimum evidence package for reviewers

To make review outcomes unambiguous, require reviewers to attach:

1. `mosquitto_sub` capture showing BaSyx emits at least one `.../updated` or `.../patched` event topic. ([wiki.basyx.org][2])
2. Agent logs showing:

   * event ingestion
   * delta publish
   * delta receive
   * merge apply
3. Before and after `$value` JSON from both sites, diffed and identical.
4. Partition test transcript (toxiproxy on, then off) showing convergence after heal. ([GitHub][6])
5. A short note explaining how identifier encoding was validated (base64url ids, URL-encoded idShortPath). ([industrialdigitaltwin.io][1])

---

## 8. Reviewer-focused "what to verify in unit tests"

A publish-ready repo should include (and reviewers should run) these tests:

### 8.1 Encoding tests

* base64url no padding for Identifiable ids
* idShortPath URL encoding, with `[index]` brackets preserved ([IDTA][12])

### 8.2 CRDT algebra tests

* join is commutative, associative, idempotent
* remove-wins or observed-remove semantics behave correctly under concurrent add/remove

### 8.3 Delta replication tests (delta-CRDT style)

* applying delta then merging is equivalent to full-state merge
* anti-entropy eventually transmits missing deltas and converges ([arXiv][4])

### 8.4 Clock tests

* HLC monotonicity under skew and message causality inputs ([cse.buffalo.edu][10])

---

## 9. Suggested "review mode" configuration settings

To make debugging deterministic, ship a `configs/review/agent.toml` profile with:

* Single-threaded apply loop (optional) for deterministic logs
* Verbose logging:

  * `ingress.event_parsed`
  * `delta.created`
  * `delta.published`
  * `delta.received`
  * `crdt.join_applied`
  * `aas.patch_sent`
  * `aas.patch_failed`
* Metrics enabled with:

  * `deltasync_delta_bytes_total`
  * `deltasync_delta_count_total`
  * `deltasync_merge_conflicts_total`
  * `deltasync_replica_digest` (label by doc id)
  * `deltasync_staleness_seconds` (label by property path)

---

If you want, I can also produce a ready-to-commit version of this document tailored to your exact crate/module names once you finalize the repo structure (even if you keep the architecture above, reviewers benefit from path-accurate references).

[1]: https://industrialdigitaltwin.io/aas-specifications/IDTA-01002/v3.1.1/http-rest-api/http-rest-api.html?utm_source=chatgpt.com "HTTP/REST API"
[2]: https://wiki.basyx.org/en/latest/content/user_documentation/basyx_components/v2/submodel_repository/features/mqtt.html?utm_source=chatgpt.com "MQTT Eventing - Eclipse BaSyx"
[3]: https://wiki.basyx.org/en/latest/content/user_documentation/basyx_components/v1/aas-server/features/hierarchical-mqtt.html?utm_source=chatgpt.com "Hierarchical MQTT Eventing - BaSyx Wiki"
[4]: https://arxiv.org/pdf/1603.01529?utm_source=chatgpt.com "arXiv:1603.01529v1 [cs.DC] 4 Mar 2016"
[5]: https://faaast-service.readthedocs.io/_/downloads/en/v1.2.0/pdf/?utm_source=chatgpt.com "FA3ST Service"
[6]: https://github.com/Shopify/toxiproxy?utm_source=chatgpt.com "Shopify/toxiproxy: :alarm_clock: A TCP proxy to simulate ..."
[7]: https://github.com/admin-shell-io/questions-and-answers?utm_source=chatgpt.com "Asset Administration Shell Frequently Asked Questions List"
[8]: https://docs.rs/crdts?utm_source=chatgpt.com "crdts - Rust"
[9]: https://docs.rs/crdts/latest/crdts/lwwreg/struct.LWWReg.html?utm_source=chatgpt.com "LWWReg in crdts"
[10]: https://cse.buffalo.edu/~demirbas/publications/hlc.pdf?utm_source=chatgpt.com "Logical Physical Clocks"
[11]: https://industrialdigitaltwin.io/aas-specifications/IDTA-01002/v3.1.1/general.html?utm_source=chatgpt.com "General - IDTA Specs Documentation"
[12]: https://industrialdigitaltwin.org/wp-content/uploads/2021/11/Details_of_the_Asset_Administration_Shell_Part_2_V1.pdf?utm_source=chatgpt.com "Details of the Asset Administration Shell - Part 2"
