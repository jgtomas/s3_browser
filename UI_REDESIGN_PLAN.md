# S3 Downloader — Approved UI Redesign Plan

## Status

Approved for implementation. This document plans Task 16 from `PLAN.md`; it does
not authorize later product features.

## Objective

Refine the existing S3 Downloader window into a clearer native desktop workflow:

1. choose an AWS CLI profile;
2. find and select a bucket;
3. identify an object and, optionally, a version;
4. choose a local destination;
5. download and understand the result.

The implementation must remain a small GPUI/`gpui-component` application. Reuse
the existing `DownloaderApp`, `AppState`, AWS CLI module, native save dialog, and
Task 15 async/version behavior.

## Scope boundaries

### In scope

- Reorder and restyle the current Sidebar and main download workspace.
- Add a local bucket-filter input.
- Add an explicit `Load versions` button while retaining Enter in the object-key
  input.
- Add a bucket refresh command that preserves usable data and selection when safe.
- Clarify selected profile/bucket context, readiness, loading, success, and failure.
- Improve truncation, scrolling, spacing, alignment, and responsive behavior.
- Add focused state/filter tests and run the repository verification commands.

### Out of scope

- S3 object browsing, key completion, uploads, deletion, or folder navigation.
- Download history, recent destinations, previews, or file metadata requests.
- New AWS CLI operations other than reusing `list-buckets` for refresh.
- Reading or displaying credentials, resolving `role_arn`, or claiming that a role
  was assumed without trustworthy runtime evidence.
- AWS SDK adoption, configuration persistence, analytics, or a new architecture.
- New icon assets or a bespoke design-system layer.

## Existing implementation to preserve

- `src/ui/main_window.rs` already owns the retained `InputState` and `SelectState`
  entities and starts background CLI work from user events.
- `src/app.rs` already owns profile, bucket, version, destination, download, and
  stale-version-response transitions.
- `src/models/app_state.rs` already validates `DownloadRequest` and derives a
  suggested destination filename.
- `src/aws/cli.rs` already builds and runs the required AWS CLI commands without a
  shell.
- `src/main.rs` already initializes GPUI Component, supplies `Root`, configures the
  native window, and preserves the macOS Quit command.

Do not move these responsibilities or introduce a new feature/service layer.

## Product and interaction decisions

### Sidebar

Use the existing `Sidebar::left()` shell and keep the Sidebar visually subordinate
to the main workspace.

Order its contents as follows:

1. App identity: `S3 Downloader` and muted `AWS object downloads` subtitle.
2. Visible `AWS profile` label and existing profile `Select`.
3. Quiet helper/status text. Use factual copy such as `AWS CLI profile active` or
   `Role credentials are resolved by AWS CLI`; do not display `Connected via
   assumed role` unless later code can prove that state.
4. `Buckets` section header with the total count and a labeled, quiet `Refresh`
   Button. The installed icon catalogue has no dedicated refresh glyph, so do not
   substitute a misleading icon or add an asset for this task.
5. Bucket filter `Input`, prefixed with `IconName::Search`, placeholder `Filter
   buckets`, and a clear affordance when supported by the installed API.
6. Existing `SidebarMenu` bucket rows with a persistent active state derived from
   `selected_bucket`.
7. Quiet footer summary such as `211 buckets loaded`.

Behavior requirements:

- The filter is case-insensitive, trims surrounding whitespace, and matches bucket
  substrings.
- Filtering is local and never invokes AWS.
- Preserve the original sorted bucket order.
- If the selected bucket does not match the current filter, keep the domain
  selection and show a quiet explanation such as `Selected bucket is hidden by the
  filter`; clearing the filter reveals it again.
- A filter with no matches shows `No buckets match “<query>”` and does not alter
  bucket state.
- Initial loading may replace the empty list with a spinner row. Refresh loading
  must retain already loaded rows and show progress on or beside `Refresh`.
- Long names truncate within the row without widening or horizontally scrolling the
  Sidebar. Preserve stable row identity from the bucket name.
- When bucket listing is denied and manual entry is enabled, retain the existing
  manual-bucket fallback. Replace the filter/list region with a clearly labeled
  `Bucket name` input and explanatory text; do not imply that filtering applies.

The installed `Badge` is an overlay notification/count component, not the neutral
inline pill shown in the concept image. Render the bucket count as restrained text
or another existing neutral composition instead of misusing `Badge`.

### Main workspace

Keep one vertically scrollable workspace with a readable maximum content width and
shared leading/trailing alignment spines. The workspace consumes surplus width;
controls must not stretch arbitrarily across very wide windows.

Compose these regions:

1. Header: `Download object` with `Choose an object and save it to your Mac.`
2. Context strip: folder icon, selected bucket, and selected profile in a quiet flat
   surface. With no bucket, show a concise selection prompt instead of an empty
   decorative container.
3. `Object` section with aligned field rows:
   - `Object key`: existing input plus `Enter the full key, including folders.`
   - `Version`: existing version `Select`, placeholder `Current version`, a nearby
     muted `Optional` label, and a visible `Load versions` Button.
   - `Save to`: existing destination input and outline `Choose…` Button.
4. File summary: derive the filename with `AppState::suggested_filename()` and show
   whether the current or a selected version will be downloaded. Do not invent size,
   content type, or remote metadata.
5. Action/status band: quiet readiness text at the leading edge and the primary
   `Download` Button at the trailing edge.

Use text-only `Download`; the installed icon catalogue has no download glyph. Keep
it as the only primary Button. `Load versions`, `Refresh`, and `Choose…` are
ordinary, ghost, or outline Buttons according to their local hierarchy.

### Version discovery

- Clicking `Load versions` and pressing Enter in the object-key field call the same
  `start_version_load` method.
- Disable `Load versions` until a profile, bucket, and nonblank key exist, and while
  a lookup or download conflicts with it.
- While loading, preserve context, prevent duplicate requests, and expose a spinner
  or Button loading state.
- Keep `Current version` as the no-selection behavior; it must continue to omit
  `--version-id`.
- Keep stable version IDs as Select values and retain current stale-response
  rejection.

### Bucket refresh

Add the smallest state transition needed to refresh the active profile:

- Add `DownloaderApp::begin_bucket_refresh` (or an equally small named method) that
  validates the selected profile, rejects duplicate refreshes, sets
  `loading_buckets`, and returns the profile needed by the existing async call.
- Reuse the existing background-executor path and `aws::cli::list_buckets`.
- On success, reconcile the current selection: retain it if the bucket still
  exists; otherwise clear the bucket and incompatible object/version state.
- On refresh failure, preserve a previously loaded usable bucket list and selection,
  surface the error, and let the user retry. Initial-load failure retains the current
  manual-entry behavior.
- Disable `Refresh` when no profile is selected or a bucket request is already in
  progress.
- A profile change still clears incompatible state as it does today.

Do not add a new AWS client, task manager, cache, or request abstraction.

### Feedback policy

- Routine prompts, counts, and readiness are muted inline text, not Alerts.
- Use `Alert::error` for actionable AWS, validation, and file errors near the action
  band.
- A completed download may use `Alert::success` because the resulting file is
  outside the app; include the destination path.
- During download, keep the form values visible, disable conflicting controls, and
  use the existing Button loading state.
- Do not encode status by color alone; pair color with icon and/or text.

Suggested readiness copy:

| State | Copy |
| --- | --- |
| No profile | `Choose an AWS profile` |
| Loading buckets | `Loading buckets…` |
| No bucket | `Choose a bucket` |
| Missing key | `Enter an object key` |
| Missing destination | `Choose where to save the file` |
| Ready | `Ready to download` |
| Downloading | `Downloading…` |

### Keyboard, focus, and accessibility

- Preserve standard keyboard behavior supplied by `Input`, `Select`, `Button`, and
  `SidebarMenu`; do not replace them with clickable `div`s.
- Tab order follows the workflow: profile, filter/refresh, bucket selection, object
  key, version selection/load, destination/choose, Download.
- Enter in object key loads versions exactly once. Enter on the focused Download
  Button invokes the same download command as a pointer click.
- Icon-only controls are not required. If one remains after API review, it must have
  an accessible name and tooltip.
- Keep focus rings visible; do not place focused controls under an ancestor that
  clips the ring.
- Disabled, loading, selected, error, and success states must remain distinct in
  both light and dark themes.

## State and ownership

- Bucket filter text is transient view state owned by `MainWindow` through one new
  retained `InputState`; it does not belong in `AppState`.
- Profiles, buckets, selected bucket, versions, selected version, destination, and
  operation status remain owned by `DownloaderApp`/`AppState`.
- Filtered buckets are derived during rendering or by a small pure helper from the
  current filter value and `state.buckets`; do not store a second bucket collection.
- The existing input/select entities remain retained and must not be recreated in
  `render`.
- Repeated bucket elements continue to use bucket names as stable domain identity.
- Render helpers may be extracted inside `main_window.rs` for named regions, but do
  not create new modules solely to shorten builder chains.

## Expected implementation files

Primary changes:

- `src/ui/main_window.rs`
  - add retained bucket-filter input and subscription;
  - compose the revised Sidebar and workspace;
  - expose `Refresh` and `Load versions` actions;
  - derive filtered buckets, context, file summary, readiness, and feedback.
- `src/app.rs`
  - add the small bucket-refresh transition;
  - reconcile bucket selection on refresh and preserve usable data on refresh
    failure.

Change only if a focused test or state invariant requires it:

- `src/models/app_state.rs`
- `src/main.rs` for a justified default/minimum window-size adjustment.

Do not change `src/aws/cli.rs`, profile parsing, packaging, or release scripts unless
a regression is discovered and separately approved.

## Luna-agent execution

All delegated implementation and review agents use `gpt-5.6-luna`. Use `high`
reasoning for code changes and `medium` for bounded inspection/verification. The
coordinator owns integration and final reporting.

Do not let multiple agents edit `src/ui/main_window.rs` concurrently. The screen is
one stateful composition and parallel edits would create needless conflict.

### Wave 1 — Parallel inspection and behavior work

#### Luna A — Component/API audit (read-only)

Scope:

- Read `AGENTS.md`, Task 16 in `PLAN.md`, this plan, the GPUI Component design and
  coding guides, and the current `main_window.rs`.
- Inspect the installed `gpui-component 0.5.1` source or official component docs for
  the exact `Input`, `Button`, `Select`, `Sidebar`, `Alert`, and Spinner APIs needed.
- Confirm available icons and report any mockup detail that cannot be expressed
  without a custom primitive.
- Report exact API findings to the coordinator. Make no file changes.

Completion gate: the UI implementer has verified signatures and a list of existing
components to compose; no API is guessed from React or a newer crate version.

#### Luna B — Bucket refresh state transition

Owned file: `src/app.rs` and focused tests colocated there if practical.

Scope:

- Implement the refresh behavior specified above without changing AWS command
  construction.
- Preserve existing profile-change and manual-entry behavior.
- Add focused tests for selection reconciliation and refresh failure preservation
  using the lightest existing GPUI test pattern available. If the repository lacks
  a practical context test harness, isolate only the reconciliation rule as a small
  pure helper and test that helper; do not introduce a test framework.
- Run `cargo fmt` and the focused test target.

Completion gate: refresh can begin only for the current profile, cannot duplicate an
in-flight request, preserves usable data on failure, and clears incompatible state
only when the selected bucket disappears.

### Wave 2 — UI implementation

#### Luna C — Main window redesign

Dependency: Luna A's API report and Luna B's completed behavior change.

Owned file: `src/ui/main_window.rs`; `src/main.rs` only if window constraints need a
small adjustment.

Scope:

- Implement the approved Sidebar and workspace composition.
- Add the retained local filter input and a pure case-insensitive filter helper.
- Wire `Refresh`, `Load versions`, `Choose…`, and `Download` to the existing named
  methods/state transitions.
- Preserve manual bucket entry, stale version handling, native destination dialog,
  and download semantics.
- Use current theme tokens, rem-based layout helpers, semantic component sizes, and
  stable IDs. Do not add raw colors or ordinary layout `px(...)` values.
- Add focused pure tests for bucket filtering and any derived readiness helper.
- Run `cargo fmt`, focused tests, and `cargo check`.

Completion gate: every normal, empty, loading, error, manual-entry, and success state
has an understandable rendering and the normal ready state matches the approved
proposal's hierarchy.

### Wave 3 — Independent verification

#### Luna D — Acceptance and regression review

Default mode is read-only. Do not edit unless the coordinator assigns a specific
finding.

Scope:

- Read the final diff against Task 16 and this plan.
- Check for invented APIs, duplicate state, raw colors/pixel layout, unstable IDs,
  misleading role status, custom clickable surfaces, and clipped focus rings.
- Run the required automated verification.
- Manually launch the app when the local AWS/macOS environment permits and exercise
  the scenario matrix below.
- Return findings by severity with file and line references. If there are no
  actionable findings, say so explicitly.

The coordinator resolves findings, reruns affected checks, and reports changed
files and remaining limitations.

## Test plan

### Automated

Add or update focused tests for:

- case-insensitive bucket substring filtering;
- trimming the filter query;
- empty query returning every bucket in original order;
- no-match filtering not changing `selected_bucket`;
- successful refresh retaining a selected bucket that still exists;
- successful refresh clearing bucket/object/version state when the bucket vanished;
- failed refresh retaining a previously usable list and selection;
- initial AccessDenied still enabling manual bucket entry;
- readiness derivation for missing profile, bucket, key, destination, and ready
  states, if readiness is extracted as a pure helper.

Do not add tests that call live AWS services or require real credentials.

### Manual scenario matrix

1. No AWS profiles: selector disabled, useful error/empty copy, main form unavailable.
2. Profile selected, buckets loading: progress visible, duplicate refresh prevented.
3. Hundreds of buckets: Sidebar scrolls, filter stays available, long names truncate.
4. Filter match/no match: order remains stable and domain selection is not mutated.
5. Selected bucket hidden by filter: selection remains and the UI explains it.
6. AccessDenied listing buckets: manual bucket entry remains usable.
7. Object key entered: Enter and `Load versions` each start one lookup.
8. Version loading/empty/error/success: current-version download remains available
   whenever existing semantics allow it.
9. Destination chooser cancelled: previous form state remains intact.
10. Ready/download/download failure/download success: state and feedback stay clear.
11. Light and dark appearance: text, borders, selection, focus, and status contrast.
12. Minimum/default/wide window: Sidebar and form remain usable without accidental
    whole-window or nested horizontal scrolling.
13. Keyboard-only pass: logical Tab order, visible focus, Select behavior, and Enter
    commands.
14. Regression: native Quit menu/Command+Q and packaged AWS CLI lookup still work.

## Required verification

Run after all implementation and fixes:

```text
cargo fmt
cargo test
cargo check
```

Also run when the local macOS toolchain permits:

```text
cargo clippy
cargo build --release
./scripts/build-macos.sh
```

Record any command that could not run and the concrete environmental reason. A
successful compile alone does not satisfy the UI acceptance criteria.

## Final acceptance criteria

- The visual reading order is profile → bucket → object → version → destination →
  download.
- The profile context is prominent and truthful without pretending to verify role
  assumption.
- Users can filter a large loaded bucket list locally, refresh it explicitly, and
  understand selection, count, loading, empty, and failure states.
- A selected bucket is persistently highlighted and repeated in the main context
  strip with the active profile.
- Version lookup is discoverable through `Load versions` and remains keyboard-fast
  through Enter in the key input.
- The file summary contains only locally derivable facts.
- Routine information is quiet; errors and asynchronous download completion use
  semantic feedback.
- `Download` is the only primary action and is enabled exactly when the existing
  validated request can be built.
- Manual bucket entry, current-version download, specific-version download, stale
  response rejection, native destination picking, and AWS error recovery still work.
- Theme tokens, rem-based spacing, component sizes, stable IDs, focus behavior, and
  scroll ownership follow GPUI Component guidance.
- No out-of-scope AWS or product feature is introduced.
- Required formatting, tests, and checks pass, with manual evidence or explicit
  remaining limitations reported.
