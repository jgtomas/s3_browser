# S3 Desktop Downloader — Implementation Plan

## Goal

Build a small native macOS desktop application written in Rust using
`gpui-component`.

The application is a graphical wrapper around AWS CLI commands.

The MVP must allow a user to:

1. Load AWS profiles from `~/.aws/config`.
2. Select an AWS profile.
3. Load S3 buckets available for that profile.
4. Select a bucket.
5. Enter an S3 object key.
6. Optionally enter an S3 Version ID.
7. Choose the destination file.
8. Download the object using AWS CLI.
9. Display success or failure feedback.

Do NOT use the AWS Rust SDK in this version.

AWS interaction must be implemented using the locally installed `aws` CLI.

---

# Technical constraints

Language:

- Rust

UI:

- GPUI
- gpui-component

Platform:

- macOS first

AWS integration:

- Execute AWS CLI using `std::process::Command`.
- Do not invoke commands through `sh -c`.
- Pass every CLI argument independently to `Command::arg`.
- Capture stdout, stderr and exit status.

Configuration:

- Read AWS profiles from `~/.aws/config`.

Keep the implementation deliberately small.

Do not introduce:

- AWS SDK for Rust
- Tokio unless absolutely required by GPUI integration
- database
- configuration persistence
- dependency injection frameworks
- complex architecture
- S3 object browser
- credential management

---

# Suggested project structure

src/
├── main.rs
├── app.rs
├── aws/
│   ├── mod.rs
│   ├── cli.rs
│   └── profiles.rs
├── models/
│   ├── mod.rs
│   └── app_state.rs
└── ui/
    ├── mod.rs
    └── main_window.rs

Responsibilities:

main.rs

- Application bootstrap.
- Initialize GPUI Component.
- Create the main window.

app.rs

- Root application state.
- Coordinate UI actions with AWS services.

aws/profiles.rs

- Locate `~/.aws/config`.
- Parse profile names.
- Return normalized profile names.

aws/cli.rs

- Check whether AWS CLI exists.
- List S3 buckets.
- Download S3 objects.

models/app_state.rs

- Selected profile.
- Available profiles.
- Selected bucket.
- Available buckets.
- Object key.
- Version ID.
- Destination path.
- Loading state.
- Error/success state.

ui/main_window.rs

- Render application UI.
- Bind controls to state.
- Trigger application actions.

---

# Task 1 — Bootstrap Rust + GPUI Component

Create the Rust project.

Configure:

- gpui
- gpui-component
- any minimal supporting dependencies required

Create a native application window.

Window title:

    S3 Downloader

Display a simple placeholder:

    S3 Downloader

Acceptance criteria:

- `cargo build` succeeds.
- `cargo run` opens a native macOS window.
- GPUI Component is initialized correctly.
- No AWS functionality yet.

Do not implement later tasks.

---

# Task 2 — AWS profile parser

Implement:

    aws/profiles.rs

Read:

    ~/.aws/config

Recognize sections such as:

    [default]

    [profile mabel_staging]

    [profile production]

Return:

    [
      "default",
      "mabel_staging",
      "production"
    ]

Requirements:

- Expand the user's home directory.
- Ignore unrelated sections.
- Remove the `profile ` prefix.
- Sort profile names.
- Do not read credentials.
- Do not modify AWS configuration.

Provide unit tests using temporary config contents.

Acceptance criteria:

- Profiles are extracted correctly.
- `default` works correctly.
- `[profile foo]` becomes `foo`.
- malformed or unrelated sections are ignored.
- Missing config returns a useful error.

Do not implement UI changes beyond what is necessary to compile.

---

# Task 3 — AWS CLI wrapper

Create:

    aws/cli.rs

Implement:

    check_aws_cli()

    list_buckets(profile: &str)

    download_object(...)

Use:

    std::process::Command

Never use:

    sh -c
    bash -c

## List buckets

Equivalent command:

    aws s3api list-buckets
      --profile PROFILE
      --query "Buckets[].Name"
      --output json
      --no-cli-pager

Parse stdout into:

    Vec<String>

## Download object

Inputs:

- profile
- bucket
- key
- optional version_id
- destination

Equivalent command:

    aws s3api get-object
      --bucket BUCKET
      --key KEY
      [--version-id VERSION_ID]
      --profile PROFILE
      DESTINATION
      --no-cli-pager

The `--version-id` argument must only be included when provided.

Return useful application errors containing stderr when AWS CLI exits
unsuccessfully.

Acceptance criteria:

- No shell interpolation is used.
- Arguments containing spaces work safely.
- AWS command failures return readable messages.
- JSON output from `list-buckets` is parsed.
- Unit tests cover command argument construction where practical.

Do not implement UI yet.

---

# Task 4 — Application state

Implement the application state required by the UI.

State should contain approximately:

    profiles: Vec<String>
    selected_profile: Option<String>

    buckets: Vec<String>
    selected_bucket: Option<String>

    object_key: String
    version_id: String
    destination: String

    loading_buckets: bool
    downloading: bool

    status: Option<AppStatus>

Where AppStatus can represent:

- success
- error
- informational state

Acceptance criteria:

- State has sensible defaults.
- AWS-specific state is not embedded inside UI components.
- Avoid unnecessary abstractions.

---

# Task 5 — Profile selector

Replace the placeholder UI.

Create the main application form.

First field:

    AWS Profile

Use a gpui-component selector/dropdown.

On application startup:

- load profiles from ~/.aws/config
- populate the selector

When the user selects a profile:

- store the selected profile
- clear previous bucket selection
- initiate bucket loading

During bucket loading display a loading indicator.

Acceptance criteria:

- AWS profiles appear in the UI.
- Selecting a profile updates application state.
- Profile loading errors appear in the UI.

---

# Task 6 — Bucket selector

Add:

    S3 Bucket

When the selected AWS profile changes, execute:

    aws s3api list-buckets ...

Populate the bucket selector.

Requirements:

- sort buckets alphabetically
- show loading state
- disable selector while loading
- show AWS CLI errors to the user
- selecting a bucket updates application state

Acceptance criteria:

- buckets associated with the selected profile are displayed
- switching profile reloads buckets
- stale buckets from the old profile are removed

---

# Task 7 — Object download form

Add fields:

    Object Key
    Version ID (optional)
    Destination

Add button:

    Download

Validation:

Profile must be selected.

Bucket must be selected.

Object Key must not be empty.

Destination must not be empty.

Version ID may be empty.

Download button should be disabled while a download is running.

Acceptance criteria:

- form values update application state
- invalid forms cannot trigger a download
- Version ID remains optional

---

# Task 8 — File destination picker

Add a native save-file dialog.

The user should be able to choose where the downloaded object will be
stored.

When possible, derive a default filename from the last portion of the S3
key.

Example:

    key:
    9e0b1d07/foo/data.json

Default filename:

    data.json

Acceptance criteria:

- Browse button opens the macOS save-file dialog.
- Selected destination appears in the form.
- User can change it before downloading.

Keep the implementation minimal.

---

# Task 9 — Execute download

Connect the Download button to:

    aws::cli::download_object()

Show progress state:

    Downloading...

On success display:

    Download completed

And the destination path.

On failure display the stderr returned by AWS CLI in a user-friendly
error component.

Do not crash the application on AWS errors.

Acceptance criteria:

- normal object download succeeds
- Version ID is passed when specified
- Version ID is omitted when empty
- AWS authentication errors are displayed
- AccessDenied errors are displayed
- missing keys are displayed as errors
- application remains usable after an error

---

# Task 10 — UX cleanup

Improve the MVP UI without changing functionality.

Suggested layout:

    S3 Downloader

    AWS Profile
    [ profile selector ]

    S3 Bucket
    [ bucket selector ]

    Object Key
    [ input ]

    Version ID (optional)
    [ input ]

    Destination
    [ input                       ] [Browse]

                           [Download]

    Status / error message

Requirements:

- reasonable macOS window size
- consistent spacing
- support GPUI Component light/dark themes
- loading indicators
- disabled controls when appropriate

Do not redesign the architecture.

---

# Task 11 — Error handling

Introduce a small application error type if useful.

Handle at least:

- ~/.aws/config does not exist
- AWS CLI not installed
- malformed AWS CLI JSON
- AWS authentication expired
- AccessDenied
- profile does not exist
- network failure
- bucket list failure
- object download failure
- invalid destination

Never expose a Rust panic to the user for expected errors.

Acceptance criteria:

- errors are shown in the UI
- errors contain enough AWS stderr information to diagnose the issue
- application continues working after failures

---

# Task 12 — Final verification

Run:

    cargo fmt --check
    cargo clippy
    cargo test
    cargo build

Manually verify:

1. Application opens.
2. AWS profiles appear.
3. Selecting profile loads buckets.
4. Selecting bucket works.
5. Object key can be entered.
6. Version ID is optional.
7. Destination can be selected.
8. Download without Version ID works.
9. Download with Version ID works.
10. AWS errors are visible and do not crash the app.

Do not add new functionality during this task.

Produce a short report containing:

- tests executed
- manual scenarios verified
- remaining known limitations

# Task 13 — macOS application packaging

Package the application as a native macOS application.

Use:

    cargo-packager

Create:

    Packager.toml

Application metadata:

    Product name: S3 Downloader
    Bundle identifier: com.julengodoy.s3downloader
    Version: read from Cargo.toml

The package must generate:

    S3 Downloader.app
    S3 Downloader.dmg

Build the application in release mode.

Target Apple Silicon initially:

    aarch64-apple-darwin

Do not introduce Intel/universal builds unless explicitly requested.

Acceptance criteria:

- `cargo build --release` succeeds.
- `cargo packager --release` succeeds.
- A `.app` bundle is generated.
- A `.dmg` installer is generated.
- The application launches after being copied from the DMG to `/Applications`.
- AWS CLI detection continues to work from the packaged application.


# Task 14 — macOS release script

Create:

    scripts/build-macos.sh

The script must:

1. Run `cargo fmt --check`.
2. Run `cargo test`.
3. Run `cargo clippy`.
4. Build release binary.
5. Package the macOS application.
6. Print the location of the generated `.dmg`.

Fail immediately if any command fails.

Use:

    set -euo pipefail

Do not include signing credentials or Apple passwords in the repository.

Signing and notarization must remain optional and configurable through
environment variables or a separate release workflow.

Acceptance criteria:

Running:

    ./scripts/build-macos.sh

produces the final DMG from a clean checkout on a correctly configured
macOS development machine.


# Task 15 — Sidebar workflow and object version selection

## Goal

Create the next visual version of S3 Downloader as a two-pane desktop workspace:

- a `gpui-component` Sidebar containing the bucket list and the AWS profile selector;
- a main download area whose inputs become available after a bucket is selected;
- object-version discovery through `s3api list-object-versions`;
- optional selection of a specific object version before downloading.

The existing AWS CLI integration, native macOS Quit menu/`Command+Q` behavior, icon, and packaging flow remain in scope only as regression requirements. Do not implement future work beyond this task.

Reference the official Sidebar API when needed: <https://longbridge.github.io/gpui-component/docs/components/sidebar>.

## User workflow

1. On startup, load the configured AWS profiles as today. The profile selector is placed at the bottom of the Sidebar, inside `SidebarFooter`.
2. When the user confirms a profile selection, clear the current bucket and object/version state, then load that profile’s buckets.
3. Render the buckets as Sidebar menu items under a “Buckets” group. Show loading, empty, and error states in the Sidebar without making the main area unusable.
4. Selecting a bucket marks that item active and unlocks the object-key, version, destination, and download controls in the main area. Before a bucket is selected, those controls are visibly disabled.
5. When the user enters an object key and presses Enter, call `s3api list-object-versions` for the selected profile, bucket, and key. Do not issue a request on every keystroke.
6. Display the downloadable versions in a version selector, with the latest version clearly identified and useful metadata such as last-modified time and size. A version selector may remain empty when the object has no version history.
7. If the user selects a version, download that exact version. If the user leaves the selector empty, download the current/latest object by omitting `--version-id`, preserving the existing behavior.
8. Changing the profile, bucket, or object key clears any incompatible bucket/version selection and prevents a previous asynchronous response from overwriting the current state.

## AWS CLI implementation

Add an `ObjectVersion` domain model in the existing model modules with the minimum fields needed by the UI and downloader:

- object key;
- version ID;
- `IsLatest`;
- `LastModified`;
- size.

Add `list_object_versions(profile, bucket, key)` to the existing AWS CLI module. Construct arguments with individual `Command::arg` calls:

    aws s3api list-object-versions \
      --bucket BUCKET \
      --prefix KEY \
      --profile PROFILE \
      --output json \
      --no-cli-pager

Keep AWS CLI pagination enabled by default; do not add `--no-paginate`. Parse the JSON response in Rust, retain only entries whose `Key` exactly equals the requested key, and ignore `DeleteMarkers` because they are not downloadable object versions. The `--prefix` filter is only an optimization and must not be treated as an exact-key filter. Avoid interpolating the object key into a JMESPath query.

Sort the resulting versions with the latest version first, followed by descending `LastModified`, while preserving stable version IDs for selection. Surface malformed output and AWS command failures through the existing error/status path.

The AWS CLI operation requires `s3:ListBucketVersions`; downloading a selected version requires `s3:GetObjectVersion`, while downloading without an explicit version uses the existing `s3:GetObject` path. See the official [list-object-versions](https://docs.aws.amazon.com/cli/latest/reference/s3api/list-object-versions.html) and [get-object](https://docs.aws.amazon.com/cli/latest/reference/s3api/get-object.html) documentation.

## Application state and async behavior

Extend the existing `AppState`/`DownloaderApp` state instead of introducing a new architectural layer:

- `versions: Vec<ObjectVersion>`;
- `selected_version_id: Option<String>`;
- `loading_versions: bool`;
- a monotonically increasing version-request ID owned by `DownloaderApp` (or an equivalent existing state owner).

Replace the free-form version-ID input with the version selector. `DownloadRequest.version_id` remains `Option<String>` and is populated from `selected_version_id`; no selection must produce `None`.

Add focused state transitions for:

- beginning a version request after validating that a bucket and nonblank key are selected;
- completing a version request only when its request ID, profile, bucket, and key still match the current state;
- selecting only a version ID that belongs to the current `versions` collection;
- clearing versions and the selected version whenever the profile, bucket, or key changes;
- retaining a usable no-explicit-version download path when version discovery returns an empty list or fails, while clearly showing the discovery error/status.

Use the existing background executor pattern for the CLI call. Update the owning entity on the UI context and notify once per accepted result. A stale result must be ignored silently and must not clear or replace newer version data.

## UI composition

Rework `MainWindow` using the installed `gpui-component` API:

- use `Sidebar::left()` as the left pane;
- use `SidebarHeader`, `SidebarGroup`, `SidebarMenu`, `SidebarMenuItem`, and `SidebarFooter` for the Sidebar structure;
- keep the selected bucket as domain state and derive each menu item’s `.active(...)` value from that state;
- put the existing profile `Select` in the Sidebar footer, with an accessible “AWS Profile” label;
- show a spinner while buckets or versions are loading;
- show clear empty/error text when no buckets or versions are available;
- keep the main download form as the existing `MainWindow`/`DownloaderApp` feature, using the standard Input, Select, Button, Alert, and Spinner components.

The bucket rows must call the existing application state transition on click. The version selector should use stable version IDs as values rather than relying on display text or list indexes. A small UI adapter type implementing the existing `SelectItem` contract is acceptable for formatting version metadata.

Disable the main object-key, version, destination, Browse, and Download controls until a bucket is selected. Keep profile and bucket navigation available while downloading, but disable actions that would create conflicting requests as appropriate. Pressing Enter in the object-key field starts exactly one version lookup for the current key. Preserve standard Select keyboard behavior and the existing native macOS menu/`Command+Q` shortcut.

Resize the default/minimum window bounds enough for a Sidebar plus the download form, while keeping the layout responsive with the existing GPUI flex/layout primitives. Use theme tokens and the component styling conventions; do not reimplement Sidebar or Select internals.

## Tests and verification

Add or update focused tests for:

- exact construction of `list-object-versions` arguments, including profiles, buckets, prefixes, and keys containing spaces or special characters;
- parsing/filtering of `Versions`, exclusion of delete markers, latest-first ordering, malformed JSON, and empty results;
- profile/bucket/key changes clearing version state;
- stale version responses being ignored when the request ID or identity no longer matches;
- valid version selection and rejection/clearing of an invalid version selection;
- download requests and command arguments with and without `--version-id`;
- disabled/enabled form behavior around bucket selection and version-loading states, where existing UI-test patterns make this practical.

Run the repository-required checks after implementation:

    cargo fmt
    cargo test
    cargo check

Also run `cargo clippy`, `cargo build --release`, and `./scripts/build-macos.sh` when the local macOS toolchain permits it. Do not require real AWS credentials or live S3 calls in automated tests. Update the README only with documentation directly required by this workflow, especially the version-list permission and the no-selection/latest-version behavior.

## Acceptance criteria

- The application presents a Sidebar with the bucket list and a profile dropdown at the bottom.
- Selecting a profile refreshes the bucket list; selecting a bucket marks it active and unlocks the main form.
- Entering a key and pressing Enter loads the exact key’s object versions through `list-object-versions`.
- The user can select a listed version and download that exact version.
- With no selected version, the download command omits `--version-id` and downloads the current/latest object.
- Loading, empty, AWS error, malformed-response, and stale-response cases are handled without corrupting current selections.
- Existing macOS Quit menu/`Command+Q`, icon, AWS CLI path handling, tests, and packaging remain working.
- The implementation changes only the files needed for Task 15 and passes the required verification commands.

# Task 16 — Approved download-workspace UI redesign

Implement the approved UI and UX redesign described in
[`UI_REDESIGN_PLAN.md`](UI_REDESIGN_PLAN.md).

This is a presentation and interaction refinement of the existing Task 15
workflow. Preserve the existing AWS CLI commands, profile parsing, object-version
semantics, download behavior, packaging, and macOS menu behavior.

Use the Luna-agent execution sequence in the linked plan. Do not implement an S3
object browser, uploads, download history, credential management, role discovery,
or configuration persistence as part of this task.

Acceptance criteria:

- the Sidebar presents the profile context before the bucket collection;
- loaded buckets can be filtered locally without making AWS requests;
- the bucket count, loading state, empty state, failure state, and persistent
  selection remain understandable;
- the selected profile and bucket are repeated as quiet context in the download
  workspace;
- object-version lookup is available through a visible `Load versions` button and
  remains available by pressing Enter in the object-key field;
- destination selection is labeled `Choose…` and still uses the native save dialog;
- routine readiness information is quiet, while errors and completed downloads
  retain clear semantic feedback;
- the `Download` button is the only primary action in the workspace;
- light and dark themes, keyboard focus, disabled/loading states, long bucket names,
  window resizing, and the manual-bucket fallback remain usable;
- the implementation follows the scope, tests, and verification gates in
  `UI_REDESIGN_PLAN.md`.
