# S3 Downloader

S3 Downloader is a small native macOS application for downloading one S3
object at a time. It uses the locally installed AWS CLI rather than the AWS
Rust SDK.

The application is written in Rust with GPUI and gpui-component.

## Features

- Reads AWS profile names from `~/.aws/config`.
- Lists buckets for the selected AWS profile.
- Downloads an object by bucket and key.
- Supports an optional S3 Version ID.
- Provides a native macOS save-file dialog.
- Shows download progress, success messages, and AWS CLI errors.
- Provides a native `S3 Downloader → Quit` menu item and `⌘Q` shortcut.
- Builds an Apple Silicon `.app` bundle and `.dmg` installer.

## Requirements

- macOS.
- Rust and Cargo. Install them with [rustup](https://rustup.rs/).
- AWS CLI v2 installed and configured.
- `cargo-packager` for creating the macOS application and DMG.
- The Apple Silicon Rust target when building the packaged release:

  ```sh
  rustup target add aarch64-apple-darwin
  ```

The application reads profile names from `~/.aws/config`. For example:

```ini
[default]
region = eu-west-1

[profile staging]
region = eu-west-1
```

Credentials are not managed by the application. Configure them using the AWS
CLI, AWS SSO, environment variables, or the normal AWS credential files. For
example:

```sh
aws configure --profile staging
aws sts get-caller-identity --profile staging
```

For an SSO profile, log in before using the application:

```sh
aws sso login --profile staging
```

## Run locally

From the project directory:

```sh
cargo run
```

Useful verification commands are:

```sh
cargo fmt --check
cargo check
cargo test
cargo clippy
cargo build --release
```

The application opens a window titled `S3 Downloader`.

## Build the macOS DMG

Install the packager once if it is not already available:

```sh
cargo install cargo-packager
```

Then run the release script:

```sh
./scripts/build-macos.sh
```

The script:

1. Checks formatting.
2. Runs the tests.
3. Runs Clippy.
4. Builds the Apple Silicon release binary.
5. Creates the `.app` bundle and `.dmg` installer.
6. Prints the generated DMG path.

The output is normally:

```text
target/aarch64-apple-darwin/release/S3 Downloader_0.1.0_aarch64.dmg
```

The exact filename includes the version configured in `Packager.toml`.

To package manually instead:

```sh
cargo build --release --target aarch64-apple-darwin
cargo packager --release
```

Packaging settings are in [`Packager.toml`](Packager.toml). The current
configuration produces an Apple Silicon application with bundle identifier
`com.julengodoy.s3downloader`.

## Install the application

1. Open the generated DMG.
2. Drag `S3 Downloader.app` to `/Applications`.
3. Launch it from Finder or Launchpad.

The package is currently unsigned and not notarized. If macOS blocks the first
launch, Control-click the application, choose **Open**, and confirm the prompt.

## Application icon

The original icon is [`icon_s3_browser.png`](icon_s3_browser.png). The
packager uses the supported 512×512 derivative
[`icon_s3_browser_512.png`](icon_s3_browser_512.png), configured in
`Packager.toml`.

If the original icon is replaced, regenerate the derivative on macOS with:

```sh
sips -z 512 512 icon_s3_browser.png --out icon_s3_browser_512.png
```

Then run `./scripts/build-macos.sh` again.

## AWS CLI permissions

The selected AWS identity generally needs:

- `s3:ListAllMyBuckets` to load the bucket list.
- `s3:GetObject` to download the current object.
- `s3:GetObjectVersion` when downloading a specific version.

AWS errors such as `AccessDenied`, expired authentication, missing objects,
and network failures are displayed in the application.

## AWS CLI PATH troubleshooting

Applications launched from Finder may receive a different `PATH` than shells
launched from Terminal. The application first checks `PATH` and then checks
these common macOS AWS CLI locations:

```text
/usr/local/bin/aws
/opt/homebrew/bin/aws
/usr/local/aws-cli/v2/current/bin/aws
```

Verify the CLI from a terminal with:

```sh
command -v aws
whence -p aws
aws --version
```

If the AWS CLI works in Terminal but not in the packaged app, confirm that the
binary is executable and installed in one of the locations above, or launch
the application from a shell with the correct `PATH`.

## Project structure

```text
src/
├── main.rs                  # GPUI bootstrap and native application menu
├── app.rs                   # Application-level state transitions
├── aws/
│   ├── cli.rs               # AWS CLI command execution and parsing
│   └── profiles.rs          # ~/.aws/config profile parsing
├── models/
│   └── app_state.rs         # Form state and download validation
└── ui/
    └── main_window.rs       # GPUI form and user interactions

Packager.toml                # macOS app and DMG configuration
scripts/build-macos.sh       # Release verification and packaging script
```

## Current limitations

- The packaged release targets Apple Silicon (`aarch64-apple-darwin`) only.
- The application downloads one explicitly selected object at a time.
- It does not browse objects or manage AWS credentials.
- Signing and notarization are not configured.
