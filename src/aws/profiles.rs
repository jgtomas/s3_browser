use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

/// Returns the location of the AWS CLI configuration file for the current user.
pub fn config_path() -> Result<PathBuf, String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "Unable to locate your home directory (HOME is not set).".to_string())?;

    Ok(home.join(".aws").join("config"))
}

/// Loads profile names from the user's AWS CLI config file.
pub fn load_profiles() -> Result<Vec<String>, String> {
    let path = config_path()?;
    load_profiles_from_path(&path)
}

fn load_profiles_from_path(path: &Path) -> Result<Vec<String>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Unable to read AWS config at {}: {}", path.display(), error))?;

    Ok(parse_profiles(&contents))
}

/// Parses only AWS profile section headers from config contents.
pub fn parse_profiles(contents: &str) -> Vec<String> {
    let mut profiles = BTreeSet::new();

    for line in contents.lines() {
        let section = line.trim();
        let Some(section) = section
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            continue;
        };

        let section = section.trim();
        if section == "default" {
            profiles.insert("default".to_string());
        } else if let Some(name) = section.strip_prefix("profile ") {
            let name = name.trim();
            if !name.is_empty() {
                profiles.insert(name.to_string());
            }
        }
    }

    profiles.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{load_profiles_from_path, parse_profiles};

    #[test]
    fn parses_and_sorts_supported_profile_sections() {
        let config = r#"
[profile zeta]
region = eu-west-1

[default]
region = us-east-1

[profile alpha]
region = us-west-2

[profile zeta]
output = json
"#;

        assert_eq!(parse_profiles(config), vec!["alpha", "default", "zeta"]);
    }

    #[test]
    fn ignores_unrelated_and_malformed_sections() {
        let config = r#"
[sso-session corp]
sso_start_url = https://example.com/start

[profile]
region = us-east-1

profile missing-brackets
[profile   ]
[profile valid]
region = us-east-1
"#;

        assert_eq!(parse_profiles(config), vec!["valid"]);
    }

    #[test]
    fn loads_profiles_from_a_config_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s3-downloader-profile-test-{}-{unique}",
            std::process::id()
        ));

        fs::write(&path, "[default]\n[profile test]\n")
            .expect("temporary AWS config should be writable");
        let profiles = load_profiles_from_path(&path).expect("temporary config should parse");
        fs::remove_file(&path).expect("temporary AWS config should be removable");

        assert_eq!(profiles, vec!["default", "test"]);
    }

    #[test]
    fn reports_missing_config_path() {
        let path = PathBuf::from("/definitely/missing/s3-downloader-config");
        let error = load_profiles_from_path(&path).expect_err("missing config should fail");

        assert!(error.contains("Unable to read AWS config"));
        assert!(error.contains("s3-downloader-config"));
    }
}
