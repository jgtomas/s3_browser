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
