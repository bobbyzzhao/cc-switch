# Native OpenAI protocol compatibility fork

This branch adds an explicit, disabled-by-default capability declaration for
third-party Responses gateways that preserve OpenAI's native Codex compaction
protocol. It is based on upstream `v4.0.12`
(`51ac86f1c59abfefabadebd05ec5a5415b8ad72c`). The fork's `main` remains upstream
code; the patch is maintained on `codex/native-gpt-compat`.

## Behavior

Enable **Native OpenAI protocol compatibility** in the provider form only when
the gateway supports native compaction requests and replay of their encrypted
output. This is a protocol declaration, not an automatic capability probe.

- Exact `gpt-*` matches in the official Codex catalog retain the official model's
  speed tiers, compaction marker, context window, tools and compaction threshold.
  Explicit window and threshold overrides still apply.
- Native compaction triggers, encrypted history and upstream compaction SSE
  events pass through. Existing CC Switch summary envelopes are converted back
  to text when returning to a native backend.
- The gateway uses its own URL and credential. Default ChatGPT account identity,
  device attestation and opaque turn state are not forwarded to it. Necessary
  Codex protocol negotiation and stable client session headers are retained, so
  account-pool gateways can keep ordinary, compaction and replay requests on the
  same account.
- In Routing mode, the existing **Enable remote compaction** preference remains
  authoritative. Native compatibility does not force remote compaction, and Fast
  remains available with local compaction.
- A native-compatible model actually published in the aggregation catalog
  enables the shared client's remote compaction path. Skipped members and foreign
  route-owned catalogs do not affect this decision. Ordinary aggregation members
  continue using the existing summary bridge.
- Unknown models, aliases and non-GPT models do not inherit official speed or
  compaction identity. Chat, Anthropic and managed OAuth providers cannot acquire
  this capability through stale metadata. Responses Lite remains disabled for
  third-party models.

The Fast selector advertises a requested tier; an upstream may return a different
tier. Matching compaction markers avoid a metadata mismatch, but do not guarantee
that different accounts/backends accept one another's ciphertext, or that the
client will never compact for another reason. Unsupported native protocol errors
remain visible instead of silently replacing the request with a summary.

The mode-switch error for `ccs-<provider>/<model>` after leaving aggregation is
separate: an existing conversation still has an aggregation model selected.
Select a model from the new catalog. Such IDs are not silently rerouted to the
default provider.

## Updating the fork

Keep the official repository as `upstream` and this fork as `fork`. Fetch official
release tags and merge the selected release into the patch branch. Do not
merge development `main` merely to update a released build.

```sh
git fetch upstream --tags
git switch codex/native-gpt-compat
git branch backup/native-gpt-compat-v4.0.12
git merge --no-ff <new-release-tag>
```

Resolve any conflicts, update the base tag/hash in this document and the version
in `src-tauri/tauri.fork.conf.json`, then run:

```sh
corepack pnpm install --frozen-lockfile
corepack pnpm typecheck
corepack pnpm exec vitest run tests/components/ProviderForm.codexOfficialCompatible.test.tsx tests/components/ProviderForm.stackModels.test.tsx tests/components/ProviderForm.codexManagedAccount.test.tsx tests/components/AboutSection.updateGuard.test.tsx
corepack pnpm build:renderer
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib codex -- --test-threads=1
corepack pnpm tauri build --bundles app --config src-tauri/tauri.fork.conf.json
git push fork HEAD:codex/native-gpt-compat
```

If a macOS release build fails to load procedural macros with
`mis-aligned LINKEDIT string pool`, set
`CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none` for the build. This works around
[rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750) by retaining
symbols in build-time helpers; the application's release strip setting is kept.

The backup branch keeps the previous working release. Review the diff against
the new release before distributing its build. The committed fork config gives
the app a version such as `4.0.12-native-openai.1`; the UI and backend use that
marker to keep checking the official feed while refusing to install an official
bundle over the fork. Clicking the update action shows a message asking ChatGPT
to rebase or merge the patch onto the latest official release and rebuild it.
Fork builds are not official signed updater artifacts. No automatic deployment
or application restart is performed by these commands.

Update checks compare fork builds against their official release baseline:
`4.0.12-native-openai.1` is current when the official feed reports `4.0.12`.
A higher official release, such as `4.0.13`, still triggers the normal reminder
and the fork installation guard. Other builds keep standard version comparison.

## Upstream submission

The patch is opt-in and preserves ordinary providers' behavior. The source and
tests contain no personal endpoints, provider IDs or credentials. Evaluate its
continued need against user outcomes: concurrent work and personal providers,
usable Fast requests, native compaction with successful continuation, and correct
history handling when switching. Upstream may satisfy these outcomes with a
different configuration or protocol implementation; it need not adopt this
patch's flag names, metadata layout or compaction marker.

Review this fork's assumptions and side effects as well. A visible Fast selector
does not prove priority service, and matching markers do not prove ciphertext
compatibility. Keep proxy behavior, upstream acceptance and successful client
continuation separate in validation reports. Remove or revise obsolete patches
when supported upstream behavior meets the actual need, and return to the
official build when no functional gap remains.
