use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppStatus {
    Info(String),
    Success(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub profile: String,
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    pub key: String,
    pub version_id: String,
    pub is_latest: bool,
    pub last_modified: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub profiles: Vec<String>,
    pub selected_profile: Option<String>,
    pub buckets: Vec<String>,
    pub selected_bucket: Option<String>,
    pub manual_bucket_entry: bool,
    pub object_key: String,
    pub versions: Vec<ObjectVersion>,
    pub selected_version_id: Option<String>,
    pub destination: String,
    pub loading_buckets: bool,
    pub loading_versions: bool,
    pub downloading: bool,
    pub status: Option<AppStatus>,
}

impl AppState {
    pub fn download_request(&self) -> Result<DownloadRequest, String> {
        let profile = self
            .selected_profile
            .as_deref()
            .filter(|profile| !profile.trim().is_empty())
            .ok_or_else(|| "Select an AWS profile before downloading.".to_string())?;
        let bucket = self
            .selected_bucket
            .as_deref()
            .filter(|bucket| !bucket.trim().is_empty())
            .ok_or_else(|| "Select an S3 bucket before downloading.".to_string())?;
        let key = self.object_key.trim();
        if key.is_empty() {
            return Err("Enter an S3 object key before downloading.".to_string());
        }
        let destination = self.destination.trim();
        if destination.is_empty() {
            return Err("Choose a destination file before downloading.".to_string());
        }

        let version_id = match self.selected_version_id.as_deref() {
            Some(version_id) if !version_id.trim().is_empty() => {
                if !self
                    .versions
                    .iter()
                    .any(|version| version.version_id == version_id)
                {
                    return Err("The selected object version is no longer available.".to_string());
                }
                Some(version_id.to_string())
            }
            _ => None,
        };

        Ok(DownloadRequest {
            profile: profile.to_string(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            version_id,
            destination: PathBuf::from(destination),
        })
    }

    pub fn suggested_filename(&self) -> String {
        self.object_key
            .trim()
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("download")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AppState, AppStatus, ObjectVersion};

    #[test]
    fn default_state_is_empty_and_idle() {
        let state = AppState::default();

        assert!(state.profiles.is_empty());
        assert!(state.selected_profile.is_none());
        assert!(state.buckets.is_empty());
        assert!(state.selected_bucket.is_none());
        assert!(!state.manual_bucket_entry);
        assert!(state.versions.is_empty());
        assert!(state.selected_version_id.is_none());
        assert!(!state.loading_buckets);
        assert!(!state.loading_versions);
        assert!(!state.downloading);
        assert!(state.status.is_none());
    }

    #[test]
    fn validates_and_builds_a_download_request() {
        let state = AppState {
            selected_profile: Some("default".to_string()),
            selected_bucket: Some("bucket".to_string()),
            object_key: " folder/object.txt ".to_string(),
            versions: vec![ObjectVersion {
                key: "folder/object.txt".to_string(),
                version_id: "version-1".to_string(),
                is_latest: false,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                size: 10,
            }],
            selected_version_id: Some("version-1".to_string()),
            destination: " /tmp/object.txt ".to_string(),
            ..Default::default()
        };

        let request = state
            .download_request()
            .expect("complete state should create a request");
        assert_eq!(request.profile, "default");
        assert_eq!(request.bucket, "bucket");
        assert_eq!(request.key, "folder/object.txt");
        assert_eq!(request.version_id.as_deref(), Some("version-1"));
        assert_eq!(request.destination, PathBuf::from("/tmp/object.txt"));
    }

    #[test]
    fn rejects_missing_download_fields() {
        let state = AppState::default();
        assert!(state.download_request().is_err());

        let state = AppState {
            selected_profile: Some("default".to_string()),
            selected_bucket: Some("bucket".to_string()),
            ..Default::default()
        };
        assert!(state.download_request().is_err());
    }

    #[test]
    fn treats_a_blank_version_as_optional() {
        let state = AppState {
            selected_profile: Some("default".to_string()),
            selected_bucket: Some("bucket".to_string()),
            object_key: "object.txt".to_string(),
            destination: "download.txt".to_string(),
            ..Default::default()
        };

        assert_eq!(state.download_request().unwrap().version_id, None);
    }

    #[test]
    fn suggests_the_last_s3_key_segment() {
        let mut state = AppState {
            object_key: "folder/nested/report.pdf".to_string(),
            ..Default::default()
        };
        assert_eq!(state.suggested_filename(), "report.pdf");

        state.object_key = "folder/".to_string();
        assert_eq!(state.suggested_filename(), "download");
    }

    #[test]
    fn status_variants_are_distinct() {
        assert_ne!(
            AppStatus::Info("message".to_string()),
            AppStatus::Success("message".to_string())
        );
    }
}
