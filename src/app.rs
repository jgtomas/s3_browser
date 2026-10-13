use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::Context;

use crate::aws;
use crate::models::{
    AppState, AppStatus, DownloadRequest, ObjectVersion, RecentObject, RecentObjectsState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionLoadRequest {
    pub profile: String,
    pub bucket: String,
    pub key: String,
    pub request_id: u64,
}

#[derive(Debug, Clone)]
pub struct RecentObjectsLoadRequest {
    pub profile: String,
    pub bucket: String,
    pub request_id: u64,
    pub cancelled: Arc<AtomicBool>,
}

fn should_start_recent_load(
    state: &RecentObjectsState,
    profile: &str,
    bucket: &str,
    force: bool,
) -> bool {
    !state.is_loading() && (force || !state.matches(profile, bucket))
}

fn recent_request_is_current(
    state: &RecentObjectsState,
    request: &RecentObjectsLoadRequest,
) -> bool {
    matches!(
        state,
        RecentObjectsState::Loading {
            profile,
            bucket,
            request_id,
        } if profile == &request.profile && bucket == &request.bucket && *request_id == request.request_id
    )
}

fn apply_recent_objects_result(
    state: &mut AppState,
    request: &RecentObjectsLoadRequest,
    result: Result<Vec<RecentObject>, String>,
) -> bool {
    if !recent_request_is_current(&state.recent_objects, request) {
        return false;
    }

    state.recent_objects = match result {
        Ok(objects) => RecentObjectsState::Loaded {
            profile: request.profile.clone(),
            bucket: request.bucket.clone(),
            request_id: request.request_id,
            objects,
        },
        Err(error) => RecentObjectsState::Failed {
            profile: request.profile.clone(),
            bucket: request.bucket.clone(),
            request_id: request.request_id,
            error,
        },
    };
    true
}

#[derive(Debug, PartialEq, Eq)]
struct BucketReconciliation {
    buckets: Vec<String>,
    selected_bucket: Option<String>,
    selected_bucket_removed: bool,
}

fn reconcile_bucket_list(
    mut buckets: Vec<String>,
    selected_bucket: Option<&str>,
) -> BucketReconciliation {
    buckets.sort_unstable();
    buckets.dedup();

    let selected_bucket = selected_bucket.map(str::to_string);
    let selected_bucket_removed = selected_bucket
        .as_deref()
        .is_some_and(|selected| !buckets.iter().any(|bucket| bucket.as_str() == selected));

    BucketReconciliation {
        buckets,
        selected_bucket: selected_bucket.filter(|_| !selected_bucket_removed),
        selected_bucket_removed,
    }
}

pub struct DownloaderApp {
    pub state: AppState,
    next_version_request_id: u64,
    next_recent_objects_request_id: u64,
    recent_objects_cancellation: Option<Arc<AtomicBool>>,
    bucket_refresh_in_progress: bool,
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
            next_recent_objects_request_id: 0,
            recent_objects_cancellation: None,
            bucket_refresh_in_progress: false,
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

        self.clear_recent_objects();
        self.state.selected_profile = Some(profile);
        self.bucket_refresh_in_progress = false;
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

    pub fn begin_bucket_refresh(&mut self, cx: &mut Context<Self>) -> Result<String, String> {
        if self.state.loading_buckets {
            let error = "A bucket refresh is already in progress.".to_string();
            self.set_error(error.clone(), cx);
            return Err(error);
        }

        let profile = self
            .state
            .selected_profile
            .as_deref()
            .filter(|profile| !profile.trim().is_empty())
            .ok_or_else(|| "Select an AWS profile before refreshing buckets.".to_string());
        let profile = match profile {
            Ok(profile) => profile,
            Err(error) => {
                self.set_error(error.clone(), cx);
                return Err(error);
            }
        };

        self.bucket_refresh_in_progress = true;
        self.state.loading_buckets = true;
        self.state.status = Some(AppStatus::Info("Refreshing buckets...".to_string()));
        cx.notify();
        Ok(profile.to_string())
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
        let is_refresh = self.bucket_refresh_in_progress;
        self.bucket_refresh_in_progress = false;
        match result {
            Ok(buckets) => {
                let reconciliation =
                    reconcile_bucket_list(buckets, self.state.selected_bucket.as_deref());
                self.state.buckets = reconciliation.buckets;
                if is_refresh {
                    self.state.selected_bucket = reconciliation.selected_bucket;
                    if reconciliation.selected_bucket_removed {
                        self.state.object_key.clear();
                        self.invalidate_version_state();
                        self.clear_recent_objects();
                    }
                    self.state.manual_bucket_entry = false;
                } else {
                    self.state.selected_bucket = None;
                }
                let message = if self.state.buckets.is_empty() {
                    "No S3 buckets were found for this profile.".to_string()
                } else {
                    format!("Found {} S3 bucket(s).", self.state.buckets.len())
                };
                self.state.status = Some(AppStatus::Info(message));
            }
            Err(error) => {
                if is_refresh {
                    self.state.status = Some(AppStatus::Error(format!(
                        "Could not refresh S3 buckets: {error}"
                    )));
                } else {
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
            self.clear_recent_objects();
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

        let bucket = (!value.trim().is_empty()).then(|| value.trim().to_string());
        if self.state.selected_bucket == bucket {
            return;
        }

        self.clear_recent_objects();
        self.state.selected_bucket = bucket;
        self.invalidate_version_state();
        cx.notify();
    }

    pub fn begin_recent_objects_load(
        &mut self,
        force: bool,
        cx: &mut Context<Self>,
    ) -> Option<RecentObjectsLoadRequest> {
        let profile = self.state.selected_profile.clone()?;
        let bucket = self.state.selected_bucket.clone()?;
        if !should_start_recent_load(&self.state.recent_objects, &profile, &bucket, force) {
            return None;
        }

        self.cancel_recent_objects_request();
        self.next_recent_objects_request_id = self.next_recent_objects_request_id.saturating_add(1);
        let request_id = self.next_recent_objects_request_id;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.state.recent_objects = RecentObjectsState::Loading {
            profile: profile.clone(),
            bucket: bucket.clone(),
            request_id,
        };
        self.recent_objects_cancellation = Some(cancelled.clone());
        cx.notify();
        Some(RecentObjectsLoadRequest {
            profile,
            bucket,
            request_id,
            cancelled,
        })
    }

    pub fn finish_recent_objects_load(
        &mut self,
        request: &RecentObjectsLoadRequest,
        result: Result<Vec<RecentObject>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !apply_recent_objects_result(&mut self.state, request, result) {
            return false;
        }

        self.recent_objects_cancellation = None;
        cx.notify();
        true
    }

    pub fn cancel_recent_objects_load(&mut self, cx: &mut Context<Self>) -> bool {
        let RecentObjectsState::Loading {
            profile,
            bucket,
            request_id,
        } = &self.state.recent_objects
        else {
            return false;
        };
        let cancelled_state = RecentObjectsState::Cancelled {
            profile: profile.clone(),
            bucket: bucket.clone(),
            request_id: *request_id,
        };
        if let Some(cancelled) = self.recent_objects_cancellation.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.state.recent_objects = cancelled_state;
        cx.notify();
        true
    }

    pub fn set_object_key(&mut self, value: String, cx: &mut Context<Self>) {
        if self.state.object_key != value {
            self.state.object_key = value;
            self.invalidate_version_state();
        }
        cx.notify();
    }

    pub fn select_recent_object_key(&mut self, key: String, cx: &mut Context<Self>) {
        self.apply_recent_object_key(key);
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

    fn apply_recent_object_key(&mut self, key: String) {
        self.state.object_key = key;
        self.invalidate_version_state();
    }

    fn cancel_recent_objects_request(&mut self) {
        if let Some(cancelled) = self.recent_objects_cancellation.take() {
            cancelled.store(true, Ordering::Release);
        }
    }

    fn clear_recent_objects(&mut self) {
        self.cancel_recent_objects_request();
        self.state.recent_objects = RecentObjectsState::Idle;
    }
}

impl Default for DownloaderApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use crate::models::RecentObjectsState;

    use super::{
        DownloaderApp, RecentObjectsLoadRequest, apply_recent_objects_result,
        recent_request_is_current, reconcile_bucket_list, should_start_recent_load,
    };

    #[test]
    fn reconciliation_sorts_deduplicates_and_retains_available_selection() {
        let reconciliation = reconcile_bucket_list(
            vec!["zeta".to_string(), "alpha".to_string(), "alpha".to_string()],
            Some("alpha"),
        );

        assert_eq!(reconciliation.buckets, vec!["alpha", "zeta"]);
        assert_eq!(reconciliation.selected_bucket.as_deref(), Some("alpha"));
        assert!(!reconciliation.selected_bucket_removed);
    }

    #[test]
    fn reconciliation_clears_selection_when_the_bucket_disappears() {
        let reconciliation =
            reconcile_bucket_list(vec!["alpha".to_string()], Some("removed-bucket"));

        assert_eq!(reconciliation.buckets, vec!["alpha"]);
        assert_eq!(reconciliation.selected_bucket, None);
        assert!(reconciliation.selected_bucket_removed);
    }

    #[test]
    fn recent_load_start_suppresses_duplicates_and_requires_force_to_refresh() {
        let loaded = RecentObjectsState::Loaded {
            profile: "default".to_string(),
            bucket: "bucket-a".to_string(),
            request_id: 1,
            objects: Vec::new(),
        };

        assert!(!should_start_recent_load(
            &loaded, "default", "bucket-a", false
        ));
        assert!(should_start_recent_load(
            &loaded, "default", "bucket-a", true
        ));
        assert!(should_start_recent_load(
            &loaded, "default", "bucket-b", false
        ));
        assert!(!should_start_recent_load(
            &RecentObjectsState::Loading {
                profile: "default".to_string(),
                bucket: "bucket-a".to_string(),
                request_id: 2,
            },
            "default",
            "bucket-a",
            true,
        ));
    }

    #[test]
    fn recent_load_completion_rejects_stale_and_reselected_bucket_requests() {
        let state = RecentObjectsState::Loading {
            profile: "default".to_string(),
            bucket: "bucket-a".to_string(),
            request_id: 3,
        };
        let request = RecentObjectsLoadRequest {
            profile: "default".to_string(),
            bucket: "bucket-a".to_string(),
            request_id: 1,
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        assert!(!recent_request_is_current(&state, &request));
    }

    #[test]
    fn recent_load_completion_preserves_a_key_typed_while_scanning() {
        let mut state = crate::models::AppState {
            object_key: "typed/during-scan.txt".to_string(),
            recent_objects: RecentObjectsState::Loading {
                profile: "default".to_string(),
                bucket: "bucket-a".to_string(),
                request_id: 3,
            },
            ..Default::default()
        };
        let request = RecentObjectsLoadRequest {
            profile: "default".to_string(),
            bucket: "bucket-a".to_string(),
            request_id: 3,
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        assert!(apply_recent_objects_result(
            &mut state,
            &request,
            Ok(Vec::new())
        ));
        assert_eq!(state.object_key, "typed/during-scan.txt");
        assert!(matches!(
            state.recent_objects,
            RecentObjectsState::Loaded { .. }
        ));
    }

    #[test]
    fn clearing_recent_objects_cancels_a_running_scan() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut application = DownloaderApp::new();
        application.recent_objects_cancellation = Some(cancellation.clone());
        application.state.recent_objects = RecentObjectsState::Loading {
            profile: "default".to_string(),
            bucket: "bucket-a".to_string(),
            request_id: 1,
        };

        application.clear_recent_objects();

        assert!(cancellation.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(application.state.recent_objects, RecentObjectsState::Idle);
    }

    #[test]
    fn selecting_a_recent_key_clears_even_the_same_keys_version_selection() {
        let mut application = DownloaderApp::new();
        application.state.object_key = "reports/latest.csv".to_string();
        application.state.versions = vec![crate::models::ObjectVersion {
            key: "reports/latest.csv".to_string(),
            version_id: "historic-version".to_string(),
            is_latest: false,
            last_modified: "2026-10-12T00:00:00Z".to_string(),
            size: 42,
        }];
        application.state.selected_version_id = Some("historic-version".to_string());
        application.state.loading_versions = true;
        let old_request_id = application.next_version_request_id;

        application.apply_recent_object_key("reports/latest.csv".to_string());

        assert_eq!(application.state.object_key, "reports/latest.csv");
        assert!(application.state.versions.is_empty());
        assert!(application.state.selected_version_id.is_none());
        assert!(!application.state.loading_versions);
        assert!(application.next_version_request_id > old_request_id);
    }
}
