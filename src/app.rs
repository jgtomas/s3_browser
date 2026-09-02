use gpui::Context;

use crate::models::{AppState, AppStatus, DownloadRequest};

pub struct DownloaderApp {
    pub state: AppState,
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
                self.state.status = Some(AppStatus::Error(format!(
                    "Could not load S3 buckets: {error}"
                )));
            }
        }
        cx.notify();
        true
    }

    pub fn select_bucket(&mut self, bucket: String, cx: &mut Context<Self>) {
        if self.state.buckets.iter().any(|item| item == &bucket) {
            self.state.selected_bucket = Some(bucket);
            cx.notify();
        }
    }

    pub fn set_object_key(&mut self, value: String, cx: &mut Context<Self>) {
        self.state.object_key = value;
        cx.notify();
    }

    pub fn set_version_id(&mut self, value: String, cx: &mut Context<Self>) {
        self.state.version_id = value;
        cx.notify();
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
}

impl Default for DownloaderApp {
    fn default() -> Self {
        Self::new()
    }
}
