use std::{
    env, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::models::ObjectVersion;

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
    use std::path::Path;

    use super::{
        download_object_command, list_buckets_command, list_object_versions_command,
        parse_bucket_names, parse_object_versions,
    };

    fn args(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
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
}
