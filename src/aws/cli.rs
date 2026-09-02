use std::{
    env, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

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

    use super::{download_object_command, list_buckets_command, parse_bucket_names};

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
}
