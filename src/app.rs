use gpui::Context;

use crate::aws;
use crate::models::{AppState, AppStatus, DownloadRequest, ObjectVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionLoadRequest {
    pub profile: String,
    pub bucket: String,
    pub key: String,
    pub request_id: u64,
}

pub struct DownloaderApp {
    pub state: AppState,
    next_version_request_id: u64,
}

impl DownloaderApp {
    pub fn new() -> Self {
        Self {
            state: AppState {
                status: Some(AppStatus::Info(
                    "Select an AWS profile to list its S3 buckets.".to_string(),
                )),
                ..Default::default()
            },
            next_version_request_id: 0,
        }
    }

    pub fn initialize(
        &mut self,
        profiles_result: Result<Vec<String>, String>,
        cli_result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let mut errors = Vec::new();

        match profiles_result {
            Ok(mut profiles) => {
                profiles.sort_unstable();
                profiles.dedup();
                self.state.profiles = profiles;
            }
            Err(error) => errors.push(error),
        }

        if let Err(error) = cli_result {
            errors.push(error);
        }

        self.state.status = if !errors.is_empty() {
            Some(AppStatus::Error(errors.join("\n\n")))
        } else if self.state.profiles.is_empty() {
            Some(AppStatus::Info(
                "No AWS profiles were found in ~/.aws/config.".to_string(),
            ))
        } else {
            Some(AppStatus::Info(
                "Select an AWS profile to list its S3 buckets.".to_string(),
            ))
        };
        cx.notify();
    }

    pub fn select_profile(&mut self, profile: String, cx: &mut Context<Self>) -> bool {
        if !self.state.profiles.iter().any(|item| item == &profile) {
            self.set_error(
                "The selected AWS profile is no longer available.".to_string(),
                cx,
            );
            return false;
        }

        self.state.selected_profile = Some(profile);
        self.state.buckets.clear();
        self.state.selected_bucket = None;
        self.state.manual_bucket_entry = false;
        self.state.object_key.clear();
        self.invalidate_version_state();
        self.state.loading_buckets = true;
        self.state.status = Some(AppStatus::Info("Loading buckets...".to_string()));
        cx.notify();
        true
    }

    pub fn finish_bucket_load(
        &mut self,
        profile: &str,
        result: Result<Vec<String>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.state.selected_profile.as_deref() != Some(profile) {
            return false;
        }

        self.state.loading_buckets = false;
        match result {
            Ok(mut buckets) => {
                buckets.sort_unstable();
                buckets.dedup();
                self.state.buckets = buckets;
                self.state.selected_bucket = None;
                let message = if self.state.buckets.is_empty() {
                    "No S3 buckets were found for this profile.".to_string()
                } else {
                    format!("Found {} S3 bucket(s).", self.state.buckets.len())
                };
                self.state.status = Some(AppStatus::Info(message));
            }
            Err(error) => {
                self.state.buckets.clear();
                self.state.selected_bucket = None;
                if aws::cli::is_access_denied_error(&error) {
                    self.state.manual_bucket_entry = true;
                    self.state.status = Some(AppStatus::Info(
                        "This profile cannot list buckets. Enter a bucket name manually."
                            .to_string(),
                    ));
                } else {
                    self.state.status = Some(AppStatus::Error(format!(
                        "Could not load S3 buckets: {error}"
                    )));
                }
            }
        }
        cx.notify();
        true
    }

    pub fn select_bucket(&mut self, bucket: String, cx: &mut Context<Self>) -> bool {
        if !self.state.manual_bucket_entry && !self.state.buckets.iter().any(|item| item == &bucket)
        {
            return false;
        }

        if self.state.selected_bucket.as_deref() != Some(bucket.as_str()) {
            self.state.selected_bucket = Some(bucket);
            self.invalidate_version_state();
            self.state.status = Some(AppStatus::Info(
                "Enter an object key and press Enter to list its versions.".to_string(),
            ));
            cx.notify();
        }
        true
    }

    pub fn set_manual_bucket(&mut self, value: String, cx: &mut Context<Self>) {
        if !self.state.manual_bucket_entry {
            return;
        }

        self.state.selected_bucket = (!value.trim().is_empty()).then(|| value.trim().to_string());
        self.invalidate_version_state();
        cx.notify();
    }

    pub fn set_object_key(&mut self, value: String, cx: &mut Context<Self>) {
        if self.state.object_key != value {
            self.state.object_key = value;
            self.invalidate_version_state();
        }
        cx.notify();
    }

    pub fn begin_version_load(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<VersionLoadRequest, String> {
        if self.state.loading_versions {
            let error = "An object version lookup is already in progress.".to_string();
            self.set_error(error.clone(), cx);
            return Err(error);
        }

        let profile = self
            .state
            .selected_profile
            .as_deref()
            .filter(|profile| !profile.trim().is_empty())
            .ok_or_else(|| "Select an AWS profile before listing object versions.".to_string());
        let profile = match profile {
            Ok(profile) => profile,
            Err(error) => {
                self.set_error(error.clone(), cx);
                return Err(error);
            }
        };

        let bucket = self
            .state
            .selected_bucket
            .as_deref()
            .filter(|bucket| !bucket.trim().is_empty())
            .ok_or_else(|| "Select an S3 bucket before listing object versions.".to_string());
        let bucket = match bucket {
            Ok(bucket) => bucket,
            Err(error) => {
                self.set_error(error.clone(), cx);
                return Err(error);
            }
        };

        let key = self.state.object_key.trim();
        if key.is_empty() {
            let error = "Enter an S3 object key before listing versions.".to_string();
            self.set_error(error.clone(), cx);
            return Err(error);
        }

        self.next_version_request_id = self.next_version_request_id.saturating_add(1);
        let request = VersionLoadRequest {
            profile: profile.to_string(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            request_id: self.next_version_request_id,
        };

        self.state.versions.clear();
        self.state.selected_version_id = None;
        self.state.loading_versions = true;
        self.state.status = Some(AppStatus::Info("Loading object versions...".to_string()));
        cx.notify();
        Ok(request)
    }

    pub fn finish_version_load(
        &mut self,
        request: &VersionLoadRequest,
        result: Result<Vec<ObjectVersion>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if request.request_id != self.next_version_request_id
            || self.state.selected_profile.as_deref() != Some(request.profile.as_str())
            || self.state.selected_bucket.as_deref() != Some(request.bucket.as_str())
            || self.state.object_key.trim() != request.key
        {
            return false;
        }

        self.state.loading_versions = false;
        self.state.selected_version_id = None;
        match result {
            Ok(versions) => {
                self.state.versions = versions;
                let message = if self.state.versions.is_empty() {
                    "No versions found for this key. Download will use the current version."
                        .to_string()
                } else {
                    format!("Found {} object version(s).", self.state.versions.len())
                };
                self.state.status = Some(AppStatus::Info(message));
            }
            Err(error) => {
                self.state.versions.clear();
                self.state.status = Some(AppStatus::Error(format!(
                    "Could not load object versions: {error}"
                )));
            }
        }
        cx.notify();
        true
    }

    pub fn select_version_id(
        &mut self,
        version_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        match version_id {
            None => {
                self.state.selected_version_id = None;
                cx.notify();
                true
            }
            Some(version_id)
                if self
                    .state
                    .versions
                    .iter()
                    .any(|version| version.version_id == version_id) =>
            {
                self.state.selected_version_id = Some(version_id);
                cx.notify();
                true
            }
            Some(_) => {
                self.state.selected_version_id = None;
                self.set_error(
                    "The selected object version is no longer available.".to_string(),
                    cx,
                );
                false
            }
        }
    }

    pub fn set_destination(&mut self, value: String, cx: &mut Context<Self>) {
        self.state.destination = value;
        cx.notify();
    }

    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.state.status = Some(AppStatus::Error(error));
        cx.notify();
    }

    pub fn begin_download(&mut self, cx: &mut Context<Self>) -> Result<DownloadRequest, String> {
        if self.state.downloading {
            let error = "A download is already in progress.".to_string();
            self.set_error(error.clone(), cx);
            return Err(error);
        }

        let request = match self.state.download_request() {
            Ok(request) => request,
            Err(error) => {
                self.set_error(error.clone(), cx);
                return Err(error);
            }
        };

        self.state.downloading = true;
        self.state.status = Some(AppStatus::Info("Downloading...".to_string()));
        cx.notify();
        Ok(request)
    }

    pub fn finish_download(
        &mut self,
        destination: String,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.state.downloading = false;
        self.state.status = Some(match result {
            Ok(()) => AppStatus::Success(format!("Download completed\n{destination}")),
            Err(error) => AppStatus::Error(format!("Download failed: {error}")),
        });
        cx.notify();
    }

    fn invalidate_version_state(&mut self) {
        self.next_version_request_id = self.next_version_request_id.saturating_add(1);
        self.state.versions.clear();
        self.state.selected_version_id = None;
        self.state.loading_versions = false;
    }
}

impl Default for DownloaderApp {
    fn default() -> Self {
        Self::new()
    }
}
