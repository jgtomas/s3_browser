use std::{
    collections::HashSet,
    env, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Utc};

use crate::models::{ObjectVersion, RecentObject};

const RECENT_OBJECT_LIMIT: usize = 5;

const AWS_CLI_FALLBACK_PATHS: &[&str] = &[
    "/usr/local/bin/aws",
    "/opt/homebrew/bin/aws",
    "/usr/local/aws-cli/v2/current/bin/aws",
];

/// Checks whether the AWS CLI can be started from the current PATH.
pub fn check_aws_cli() -> Result<(), String> {
    let output = aws_command()
        .arg("--version")
        .output()
        .map_err(|error| command_start_error(error, "checking for the AWS CLI"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure("Checking for the AWS CLI", &output))
    }
}

/// Lists S3 bucket names for an AWS profile.
pub fn list_buckets(profile: &str) -> Result<Vec<String>, String> {
    let output = run_command(
        list_buckets_command(profile),
        format!("Listing S3 buckets for profile '{profile}'"),
    )?;

    parse_bucket_names(&output.stdout)
}

/// Returns whether an AWS CLI error indicates that bucket listing is not allowed.
pub fn is_access_denied_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "accessdenied",
        "access denied",
        "not authorized",
        "unauthorizedoperation",
        "forbidden",
    ]
    .iter()
    .any(|marker| error.contains(marker))
}

/// Lists all downloadable versions of an exact S3 object key.
pub fn list_object_versions(
    profile: &str,
    bucket: &str,
    key: &str,
) -> Result<Vec<ObjectVersion>, String> {
    let output = run_command(
        list_object_versions_command(profile, bucket, key),
        format!("Listing object versions for s3://{bucket}/{key}"),
    )?;

    parse_object_versions(&output.stdout, key)
}

/// Lists the five most recently modified current objects in a bucket.
/// Returns `None` when cancellation was requested between pages.
pub fn list_recent_objects(
    profile: &str,
    bucket: &str,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<RecentObject>>, String> {
    collect_recent_objects(
        |continuation_token| {
            let output = run_command(
                list_objects_v2_command(profile, bucket, continuation_token),
                format!("Listing objects in S3 bucket '{bucket}'"),
            )?;
            parse_recent_objects_page(&output.stdout)
        },
        cancelled,
    )
}

/// Downloads one S3 object to a local destination using the AWS CLI.
pub fn download_object(
    profile: &str,
    bucket: &str,
    key: &str,
    version_id: Option<&str>,
    destination: &Path,
) -> Result<(), String> {
    let output = run_command(
        download_object_command(profile, bucket, key, version_id, destination),
        format!("Downloading s3://{bucket}/{key}"),
    )?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure("Downloading the S3 object", &output))
    }
}

fn list_buckets_command(profile: &str) -> Command {
    let mut command = aws_command();
    command
        .arg("s3api")
        .arg("list-buckets")
        .arg("--profile")
        .arg(profile)
        .arg("--query")
        .arg("Buckets[].Name")
        .arg("--output")
        .arg("json")
        .arg("--no-cli-pager");
    command
}

fn list_object_versions_command(profile: &str, bucket: &str, key: &str) -> Command {
    let mut command = aws_command();
    command
        .arg("s3api")
        .arg("list-object-versions")
        .arg("--bucket")
        .arg(bucket)
        .arg("--prefix")
        .arg(key)
        .arg("--profile")
        .arg(profile)
        .arg("--output")
        .arg("json")
        .arg("--no-cli-pager");
    command
}

fn list_objects_v2_command(
    profile: &str,
    bucket: &str,
    continuation_token: Option<&str>,
) -> Command {
    let mut command = aws_command();
    command
        .arg("s3api")
        .arg("list-objects-v2")
        .arg("--bucket")
        .arg(bucket)
        .arg("--profile")
        .arg(profile)
        .arg("--max-keys")
        .arg("1000")
        .arg("--output")
        .arg("json")
        .arg("--no-cli-pager")
        .arg("--no-paginate");

    if let Some(token) = continuation_token {
        command.arg("--continuation-token").arg(token);
    }
    command
}

fn download_object_command(
    profile: &str,
    bucket: &str,
    key: &str,
    version_id: Option<&str>,
    destination: &Path,
) -> Command {
    let mut command = aws_command();
    command
        .arg("s3api")
        .arg("get-object")
        .arg("--bucket")
        .arg(bucket)
        .arg("--key")
        .arg(key);

    if let Some(version_id) = version_id.filter(|value| !value.is_empty()) {
        command.arg("--version-id").arg(version_id);
    }

    command
        .arg("--profile")
        .arg(profile)
        .arg(destination.as_os_str())
        .arg("--no-cli-pager");
    command
}

fn aws_command() -> Command {
    Command::new(resolve_aws_cli().unwrap_or_else(|| PathBuf::from("aws")))
}

fn resolve_aws_cli() -> Option<PathBuf> {
    let from_path = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path).find_map(|directory| {
            let candidate = directory.join("aws");
            candidate.is_file().then_some(candidate)
        })
    });

    from_path.or_else(|| {
        AWS_CLI_FALLBACK_PATHS
            .iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file())
    })
}

fn run_command(mut command: Command, operation: String) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| command_start_error(error, &operation))?;

    if output.status.success() {
        Ok(output)
    } else {
        Err(command_failure(&operation, &output))
    }
}

fn parse_bucket_names(stdout: &[u8]) -> Result<Vec<String>, String> {
    let mut buckets: Vec<String> = serde_json::from_slice(stdout)
        .map_err(|error| format!("AWS CLI returned invalid bucket JSON: {error}"))?;
    buckets.sort_unstable();
    buckets.dedup();
    Ok(buckets)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecentObjectsPage {
    objects: Vec<RecentObject>,
    is_truncated: bool,
    next_continuation_token: Option<String>,
}

fn parse_recent_objects_page(stdout: &[u8]) -> Result<RecentObjectsPage, String> {
    let document: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|error| format!("AWS CLI returned invalid object listing JSON: {error}"))?;

    let contents = match document.get("Contents") {
        None | Some(serde_json::Value::Null) => &[][..],
        Some(value) => value
            .as_array()
            .ok_or_else(|| "AWS CLI object listing Contents must be an array.".to_string())?,
    };
    let mut objects = Vec::with_capacity(contents.len());
    for object in contents {
        let key = object
            .get("Key")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "AWS CLI returned an object without a Key.".to_string())?;
        let timestamp = object
            .get("LastModified")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "AWS CLI returned an object without LastModified.".to_string())?;
        let last_modified = DateTime::parse_from_rfc3339(timestamp)
            .map_err(|error| format!("AWS CLI returned an invalid LastModified value: {error}"))?
            .with_timezone(&Utc);
        objects.push(RecentObject {
            key: key.to_string(),
            last_modified,
        });
    }

    let is_truncated = document
        .get("IsTruncated")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| {
            "AWS CLI object listing is missing a valid IsTruncated value.".to_string()
        })?;
    let next_continuation_token = document
        .get("NextContinuationToken")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    Ok(RecentObjectsPage {
        objects,
        is_truncated,
        next_continuation_token,
    })
}

fn collect_recent_objects(
    mut fetch_page: impl FnMut(Option<&str>) -> Result<RecentObjectsPage, String>,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<RecentObject>>, String> {
    let mut best_objects = Vec::with_capacity(RECENT_OBJECT_LIMIT);
    let mut seen_tokens = HashSet::new();
    let mut continuation_token: Option<String> = None;

    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let page = fetch_page(continuation_token.as_deref())?;
        best_objects.extend(page.objects);
        sort_and_limit_recent_objects(&mut best_objects);
        if !page.is_truncated {
            return Ok(Some(best_objects));
        }

        let next_token = page
            .next_continuation_token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                "AWS CLI marked the object listing truncated without a continuation token."
                    .to_string()
            })?;
        if !seen_tokens.insert(next_token.clone()) {
            return Err("AWS CLI repeated an object listing continuation token.".to_string());
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        continuation_token = Some(next_token);
    }
}

fn sort_and_limit_recent_objects(objects: &mut Vec<RecentObject>) {
    objects.sort_by(|left, right| {
        right
            .last_modified
            .cmp(&left.last_modified)
            .then_with(|| left.key.cmp(&right.key))
    });
    objects.truncate(RECENT_OBJECT_LIMIT);
}

fn parse_object_versions(stdout: &[u8], requested_key: &str) -> Result<Vec<ObjectVersion>, String> {
    let document: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|error| format!("AWS CLI returned invalid object version JSON: {error}"))?;
    let versions = document
        .get("Versions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "AWS CLI object version JSON is missing a Versions array.".to_string())?;

    let mut parsed = Vec::new();
    for version in versions {
        let key = version
            .get("Key")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "AWS CLI returned an object version without a Key.".to_string())?;
        if key != requested_key {
            continue;
        }

        let version_id = version
            .get("VersionId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "AWS CLI returned an object version without a VersionId.".to_string())?;
        let is_latest = version
            .get("IsLatest")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| {
                "AWS CLI returned an object version without a valid IsLatest value.".to_string()
            })?;
        let last_modified = version
            .get("LastModified")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                "AWS CLI returned an object version without a LastModified value.".to_string()
            })?;
        let size = version
            .get("Size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                "AWS CLI returned an object version without a valid Size.".to_string()
            })?;

        parsed.push(ObjectVersion {
            key: key.to_string(),
            version_id: version_id.to_string(),
            is_latest,
            last_modified: last_modified.to_string(),
            size,
        });
    }

    parsed.sort_by(|left, right| {
        right
            .is_latest
            .cmp(&left.is_latest)
            .then_with(|| right.last_modified.cmp(&left.last_modified))
            .then_with(|| left.version_id.cmp(&right.version_id))
    });
    Ok(parsed)
}

fn command_start_error(error: io::Error, operation: &str) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        format!("The AWS CLI is not installed or not available on PATH while {operation}.")
    } else {
        format!("Unable to start the AWS CLI while {operation}: {error}")
    }
}

fn command_failure(operation: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        format!("{operation} failed: {stderr}")
    } else {
        format!("{operation} failed with status {}.", output.status)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        path::Path,
        sync::atomic::{AtomicBool, Ordering},
    };

    use super::{
        RecentObjectsPage, collect_recent_objects, download_object_command, list_buckets_command,
        list_object_versions_command, list_objects_v2_command, parse_bucket_names,
        parse_object_versions, parse_recent_objects_page,
    };

    fn args(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn recent_page(
        entries: &[(&str, &str)],
        is_truncated: bool,
        token: Option<&str>,
    ) -> RecentObjectsPage {
        let contents: Vec<_> = entries
            .iter()
            .map(|(key, timestamp)| serde_json::json!({"Key": key, "LastModified": timestamp}))
            .collect();
        let json = serde_json::json!({
            "Contents": contents,
            "IsTruncated": is_truncated,
            "NextContinuationToken": token,
        });
        parse_recent_objects_page(json.to_string().as_bytes())
            .expect("recent objects page should parse")
    }

    #[test]
    fn list_command_keeps_profile_as_one_argument() {
        let command = list_buckets_command("profile with spaces");
        let values = args(&command);

        assert_eq!(
            values[0..4],
            ["s3api", "list-buckets", "--profile", "profile with spaces"]
        );
        assert!(values.contains(&"--query".to_string()));
        assert!(values.contains(&"--output".to_string()));
    }

    #[test]
    fn list_versions_command_keeps_key_and_profile_as_one_argument() {
        let command = list_object_versions_command(
            "profile with spaces",
            "bucket-name",
            "folder/object with spaces and \"quotes\".txt",
        );
        let values = args(&command);

        assert_eq!(
            values[0..6],
            [
                "s3api",
                "list-object-versions",
                "--bucket",
                "bucket-name",
                "--prefix",
                "folder/object with spaces and \"quotes\".txt"
            ]
        );
        assert!(values.contains(&"profile with spaces".to_string()));
        assert!(!values.contains(&"--query".to_string()));
        assert!(!values.contains(&"--no-paginate".to_string()));
    }

    #[test]
    fn list_objects_command_passes_profile_bucket_and_pagination_separately() {
        let first_page = list_objects_v2_command("profile with spaces", "client-bucket", None);
        let first_args = args(&first_page);
        assert_eq!(
            first_args[0..6],
            [
                "s3api",
                "list-objects-v2",
                "--bucket",
                "client-bucket",
                "--profile",
                "profile with spaces"
            ]
        );
        assert!(
            first_args
                .windows(2)
                .any(|pair| pair == ["--max-keys", "1000"])
        );
        assert!(first_args.contains(&"--no-paginate".to_string()));
        assert!(!first_args.contains(&"--continuation-token".to_string()));

        let next_page = list_objects_v2_command("profile", "bucket", Some("token with spaces"));
        let next_args = args(&next_page);
        let token_ix = next_args
            .iter()
            .position(|arg| arg == "--continuation-token")
            .expect("continuation token should be present");
        assert_eq!(next_args[token_ix + 1], "token with spaces");
    }

    #[test]
    fn download_command_keeps_paths_and_keys_as_one_argument() {
        let command = download_object_command(
            "profile name",
            "bucket-name",
            "folder/object with spaces.txt",
            None,
            Path::new("/tmp/a destination/object.txt"),
        );
        let values = args(&command);

        assert!(values.contains(&"folder/object with spaces.txt".to_string()));
        assert!(values.contains(&"/tmp/a destination/object.txt".to_string()));
        assert!(!values.contains(&"--version-id".to_string()));
    }

    #[test]
    fn download_command_includes_a_version_when_given() {
        let command = download_object_command(
            "default",
            "bucket",
            "object",
            Some("version id"),
            Path::new("download.bin"),
        );
        let values = args(&command);
        let version_index = values
            .iter()
            .position(|value| value == "--version-id")
            .expect("version flag should be present");

        assert_eq!(values[version_index + 1], "version id");
    }

    #[test]
    fn parses_sorted_unique_bucket_names() {
        let json = br#"["zeta", "alpha", "zeta"]"#;
        assert_eq!(
            parse_bucket_names(json).expect("bucket JSON should parse"),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn recognizes_bucket_permission_errors() {
        assert!(super::is_access_denied_error(
            "An error occurred (AccessDenied) when calling ListBuckets"
        ));
        assert!(super::is_access_denied_error("User is not authorized"));
        assert!(!super::is_access_denied_error(
            "Unable to connect to the endpoint"
        ));
    }

    #[test]
    fn reports_malformed_bucket_json() {
        let error = parse_bucket_names(b"not json").expect_err("malformed JSON should fail");
        assert!(error.contains("invalid bucket JSON"));
    }

    #[test]
    fn parses_exact_object_versions_in_latest_first_order() {
        let json = br#"{
            "Versions": [
                {
                    "Key": "folder/other.txt",
                    "VersionId": "other",
                    "IsLatest": true,
                    "LastModified": "2026-02-01T00:00:00Z",
                    "Size": 5
                },
                {
                    "Key": "folder/object.txt",
                    "VersionId": "older",
                    "IsLatest": false,
                    "LastModified": "2026-01-01T00:00:00Z",
                    "Size": 12
                },
                {
                    "Key": "folder/object.txt",
                    "VersionId": "latest",
                    "IsLatest": true,
                    "LastModified": "2026-02-01T00:00:00Z",
                    "Size": 24
                },
                {
                    "Key": "folder/object.txt",
                    "VersionId": "newer",
                    "IsLatest": false,
                    "LastModified": "2026-03-01T00:00:00Z",
                    "Size": 36
                }
            ],
            "DeleteMarkers": [
                {
                    "Key": "folder/object.txt",
                    "VersionId": "deleted",
                    "IsLatest": false
                }
            ]
        }"#;

        let versions = parse_object_versions(json, "folder/object.txt")
            .expect("object version JSON should parse");

        assert_eq!(
            versions
                .iter()
                .map(|version| version.version_id.as_str())
                .collect::<Vec<_>>(),
            vec!["latest", "newer", "older"]
        );
        assert!(versions[0].is_latest);
        assert_eq!(versions[0].size, 24);
        assert!(
            versions
                .iter()
                .all(|version| version.key == "folder/object.txt")
        );
    }

    #[test]
    fn parses_empty_object_version_results() {
        let versions = parse_object_versions(br#"{"Versions": []}"#, "object.txt")
            .expect("empty object version JSON should parse");
        assert!(versions.is_empty());
    }

    #[test]
    fn reports_malformed_object_version_json() {
        let error = parse_object_versions(b"not json", "object.txt")
            .expect_err("malformed object version JSON should fail");
        assert!(error.contains("invalid object version JSON"));

        let error = parse_object_versions(br#"{}"#, "object.txt")
            .expect_err("missing Versions should fail");
        assert!(error.contains("missing a Versions array"));
    }

    #[test]
    fn recent_objects_are_globally_ordered_across_pages_and_limited_to_five() {
        let first = recent_page(
            &[
                ("old.txt", "2026-10-01T10:00:00Z"),
                ("key-4.txt", "2026-10-04T10:00:00Z"),
                ("key-2.txt", "2026-10-02T10:00:00Z"),
            ],
            true,
            Some("page 2"),
        );
        let second = recent_page(
            &[
                ("key-6.txt", "2026-10-06T10:00:00Z"),
                ("key-5.txt", "2026-10-05T10:00:00Z"),
                ("key-3.txt", "2026-10-03T10:00:00Z"),
            ],
            false,
            None,
        );
        let mut pages = VecDeque::from([first, second]);
        let mut requested_tokens = Vec::new();
        let cancelled = AtomicBool::new(false);

        let result = collect_recent_objects(
            |token| {
                requested_tokens.push(token.map(str::to_string));
                Ok(pages.pop_front().expect("page should be requested"))
            },
            &cancelled,
        )
        .expect("all pages should be collected")
        .expect("scan should not be cancelled");

        assert_eq!(requested_tokens, vec![None, Some("page 2".to_string())]);
        assert_eq!(
            result
                .iter()
                .map(|object| object.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "key-6.txt",
                "key-5.txt",
                "key-4.txt",
                "key-3.txt",
                "key-2.txt"
            ]
        );
    }

    #[test]
    fn recent_objects_keep_fewer_than_five_and_allow_empty_buckets() {
        let page = recent_page(
            &[("folder/zero-byte-marker", "2026-10-01T10:00:00Z")],
            false,
            None,
        );
        let result = collect_recent_objects(|_| Ok(page.clone()), &AtomicBool::new(false))
            .expect("listing should succeed")
            .expect("listing should finish");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "folder/zero-byte-marker");

        let empty = parse_recent_objects_page(br#"{"IsTruncated":false}"#)
            .expect("an empty bucket may omit Contents");
        assert!(empty.objects.is_empty());
    }

    #[test]
    fn recent_object_timestamp_offsets_compare_as_the_same_instant_and_ties_sort_by_key() {
        let page = recent_page(
            &[
                ("zeta.txt", "2026-10-01T10:00:00+02:00"),
                ("alpha.txt", "2026-10-01T08:00:00Z"),
                ("newer.txt", "2026-10-01T08:00:01Z"),
            ],
            false,
            None,
        );
        let result = collect_recent_objects(|_| Ok(page.clone()), &AtomicBool::new(false))
            .expect("listing should succeed")
            .expect("listing should finish");
        assert_eq!(
            result
                .iter()
                .map(|object| object.key.as_str())
                .collect::<Vec<_>>(),
            vec!["newer.txt", "alpha.txt", "zeta.txt"]
        );
        assert_eq!(result[1].last_modified, result[2].last_modified);
    }

    #[test]
    fn rejects_malformed_recent_object_pages_and_timestamps() {
        assert!(
            parse_recent_objects_page(b"not json")
                .expect_err("invalid JSON should fail")
                .contains("invalid object listing JSON")
        );
        assert!(
            parse_recent_objects_page(
                br#"{"Contents":[{"Key":"missing-date"}],"IsTruncated":false}"#
            )
            .expect_err("missing timestamp should fail")
            .contains("without LastModified")
        );
        assert!(parse_recent_objects_page(
            br#"{"Contents":[{"Key":"bad-date","LastModified":"yesterday"}],"IsTruncated":false}"#
        )
        .expect_err("invalid timestamp should fail")
        .contains("invalid LastModified"));
        assert!(
            parse_recent_objects_page(br#"{"Contents":[]}"#)
                .expect_err("missing truncation state should fail")
                .contains("IsTruncated")
        );
    }

    #[test]
    fn rejects_missing_or_repeated_recent_object_continuation_tokens() {
        let missing = recent_page(&[], true, None);
        assert!(
            collect_recent_objects(|_| Ok(missing.clone()), &AtomicBool::new(false))
                .expect_err("a truncated page requires a token")
                .contains("without a continuation token")
        );

        let repeated = recent_page(&[], true, Some("same token"));
        let mut pages = VecDeque::from([repeated.clone(), repeated]);
        assert!(
            collect_recent_objects(|_| Ok(pages.pop_front().unwrap()), &AtomicBool::new(false))
                .expect_err("a repeated token must fail")
                .contains("repeated an object listing continuation token")
        );
    }

    #[test]
    fn cancellation_stops_before_requesting_another_page() {
        let cancelled = AtomicBool::new(false);
        let mut calls = 0;
        let result = collect_recent_objects(
            |_| {
                calls += 1;
                cancelled.store(true, Ordering::Release);
                Ok(recent_page(&[], true, Some("next")))
            },
            &cancelled,
        )
        .expect("cancellation is not a listing failure");
        assert!(result.is_none());
        assert_eq!(calls, 1);

        cancelled.store(true, Ordering::Release);
        let result = collect_recent_objects(
            |_| panic!("a cancelled scan must not start a request"),
            &cancelled,
        )
        .expect("cancellation should return normally");
        assert!(result.is_none());
    }
}
