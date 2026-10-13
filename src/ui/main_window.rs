use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Sizable, StyledExt,
    alert::Alert,
    button::{Button, ButtonVariants},
    combobox::{Combobox, ComboboxEvent, ComboboxState},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    resizable::{ResizableState, h_resizable, resizable_panel},
    scroll::ScrollableElement,
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectItem, SelectState},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, Context, ElementId, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder, rems,
};

use crate::{
    NarrowSidebar, ResetSidebar, WidenSidebar,
    app::{DownloaderApp, RecentObjectsLoadRequest, VersionLoadRequest},
    aws,
    models::{
        AppState, AppStatus, DownloadRequest, ObjectVersion, RecentObject, RecentObjectsState,
    },
};

#[cfg(test)]
use gpui_kit::test::TestSupportExt as _;

const SIDEBAR_WIDTH_REMS: f32 = 22.0;
const SIDEBAR_MIN_WIDTH_REMS: f32 = 18.0;
const SIDEBAR_MAX_WIDTH_REMS: f32 = 28.0;
const WORKSPACE_MIN_WIDTH_REMS: f32 = 30.0;
const CONTENT_MAX_WIDTH_REMS: f32 = 56.0;

fn profile_selection_changed(current: Option<&str>, proposed: &str) -> bool {
    current != Some(proposed)
}

fn bucket_confirmation_enabled(loading_buckets: bool, downloading: bool) -> bool {
    !loading_buckets && !downloading
}

fn sidebar_width_max_rem(container_width_rem: f32) -> f32 {
    SIDEBAR_MAX_WIDTH_REMS
        .min((container_width_rem - WORKSPACE_MIN_WIDTH_REMS).max(SIDEBAR_MIN_WIDTH_REMS))
}

type ListBucketsCommand = dyn Fn(&str) -> Result<Vec<String>, String> + Send + Sync;
type ListRecentObjectsCommand =
    dyn Fn(&str, &str, Arc<AtomicBool>) -> Result<Option<Vec<RecentObject>>, String> + Send + Sync;
type ListVersionsCommand =
    dyn Fn(&str, &str, &str) -> Result<Vec<ObjectVersion>, String> + Send + Sync;
type DownloadCommand =
    dyn Fn(&str, &str, &str, Option<&str>, &Path) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
struct AwsCommands {
    list_buckets: Arc<ListBucketsCommand>,
    list_recent_objects: Arc<ListRecentObjectsCommand>,
    list_versions: Arc<ListVersionsCommand>,
    download: Arc<DownloadCommand>,
}

impl Default for AwsCommands {
    fn default() -> Self {
        Self {
            list_buckets: Arc::new(aws::cli::list_buckets),
            list_recent_objects: Arc::new(|profile, bucket, cancelled| {
                aws::cli::list_recent_objects(profile, bucket, &cancelled)
            }),
            list_versions: Arc::new(aws::cli::list_object_versions),
            download: Arc::new(aws::cli::download_object),
        }
    }
}

struct BucketListDelegate {
    profile: Option<String>,
    buckets: Vec<String>,
    visible_buckets: Vec<String>,
    query: String,
    confirmed_bucket: Option<String>,
    empty_message: String,
}

impl BucketListDelegate {
    fn from_state(state: &AppState) -> Self {
        let mut delegate = Self {
            profile: None,
            buckets: Vec::new(),
            visible_buckets: Vec::new(),
            query: String::new(),
            confirmed_bucket: None,
            empty_message: String::new(),
        };
        delegate.update_from_state(state);
        delegate
    }

    fn update_from_state(&mut self, state: &AppState) {
        self.profile = state.selected_profile.clone();
        self.buckets = state.buckets.clone();
        self.confirmed_bucket = state.selected_bucket.clone();
        self.empty_message = if state.loading_buckets && state.buckets.is_empty() {
            "Loading buckets…".to_string()
        } else if state.selected_profile.is_none() {
            "Select a profile to list buckets".to_string()
        } else if state.manual_bucket_entry {
            "Enter a bucket name above".to_string()
        } else if let Some(AppStatus::Error(message)) = &state.status
            && state.buckets.is_empty()
        {
            message.clone()
        } else if !self.query.is_empty() && !state.buckets.is_empty() {
            format!("No buckets match “{}”", self.query)
        } else {
            "No S3 buckets found for this profile".to_string()
        };
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        self.visible_buckets = filter_buckets(&self.buckets, &self.query);
    }

    fn bucket_at(&self, row: usize) -> Option<&String> {
        self.visible_buckets.get(row)
    }

    fn selected_bucket_is_hidden(&self) -> bool {
        self.confirmed_bucket.as_ref().is_some_and(|selected| {
            !self.query.is_empty() && !self.visible_buckets.iter().any(|bucket| bucket == selected)
        })
    }
}

impl ListDelegate for BucketListDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.trim().to_string();
        self.apply_filter();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &gpui_kit::App) -> usize {
        self.visible_buckets.len()
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let bucket = self.bucket_at(index.row)?.clone();
        let profile = self.profile.as_deref().unwrap_or("unselected");
        let row_id: ElementId = format!("bucket-row:{profile}:{bucket}").into();
        let button_id: ElementId = format!("bucket-label:{profile}:{bucket}").into();
        let icon_id: ElementId = format!("bucket-icon:{profile}:{bucket}").into();
        let label_id: ElementId = format!("bucket-name:{profile}:{bucket}").into();
        let icon = div()
            .id(icon_id)
            .child(Icon::new(IconName::Folder).size_4());
        #[cfg(test)]
        let icon = icon.test_support();
        let label = div()
            .id(label_id)
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(bucket.clone());
        #[cfg(test)]
        let label = label.test_support();
        Some(
            ListItem::new(row_id)
                .confirmed(self.confirmed_bucket.as_deref() == Some(bucket.as_str()))
                .accessibility_label(bucket.clone())
                .child(
                    Button::new(button_id)
                        .ghost()
                        .small()
                        .accessibility_label(bucket.clone())
                        .tooltip(bucket.clone())
                        .w_full()
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_start()
                                .gap_2()
                                .child(icon)
                                .child(label),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }

    fn confirm(&mut self, _: bool, _: &mut Window, _: &mut Context<ListState<Self>>) {}

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .p_3()
            .gap_2()
            .items_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .when(self.empty_message == "Loading buckets…", |this| {
                this.child(Spinner::new().xsmall())
            })
            .child(self.empty_message.clone())
    }
}

impl SelectItem for ObjectVersion {
    type Value = String;

    fn title(&self) -> SharedString {
        let label = if self.is_latest { "Latest" } else { "Version" };
        format!(
            "{label} · {} · {} · {}",
            self.version_id,
            self.last_modified,
            format_size(self.size)
        )
        .into()
    }

    fn value(&self) -> &Self::Value {
        &self.version_id
    }
}

pub struct MainWindow {
    application: Entity<DownloaderApp>,
    profile_select: Entity<ComboboxState<SearchableVec<String>>>,
    version_select: Entity<SelectState<Vec<ObjectVersion>>>,
    bucket_list: Entity<ListState<BucketListDelegate>>,
    resizable_state: Entity<ResizableState>,
    bucket_name_input: Entity<InputState>,
    object_key_input: Entity<InputState>,
    destination_input: Entity<InputState>,
    commands: AwsCommands,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profiles_result = aws::profiles::load_profiles();
        let cli_result = aws::cli::check_aws_cli();
        Self::new_with_dependencies(
            window,
            cx,
            profiles_result,
            cli_result,
            AwsCommands::default(),
            None,
        )
    }

    fn new_with_dependencies(
        window: &mut Window,
        cx: &mut Context<Self>,
        profiles_result: Result<Vec<String>, String>,
        cli_result: Result<(), String>,
        commands: AwsCommands,
        fixture_state: Option<AppState>,
    ) -> Self {
        let application = cx.new(|_| DownloaderApp::new());
        application.update(cx, |application, cx| {
            application.initialize(profiles_result.clone(), cli_result, cx);
            if let Some(state) = fixture_state {
                application.state = state;
                cx.notify();
            }
        });
        let state = application.read(cx).state.clone();
        let profile_select = cx.new(|cx| {
            ComboboxState::new(
                SearchableVec::new(state.profiles.clone()),
                Vec::new(),
                window,
                cx,
            )
            .searchable(true)
        });
        let version_select =
            cx.new(|cx| SelectState::new(Vec::<ObjectVersion>::new(), None, window, cx));
        let bucket_list = cx.new(|cx| {
            ListState::new(BucketListDelegate::from_state(&state), window, cx).searchable(true)
        });
        let resizable_state = cx.new(|_| ResizableState::default());
        let bucket_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Enter bucket name"));
        let object_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("folder/object.txt"));
        let destination_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Choose a local destination"));

        profile_select.update(cx, |picker, cx| {
            if let Some(profile) = state.selected_profile.as_ref() {
                picker.set_selected_values(std::slice::from_ref(profile), window, cx);
            }
            cx.notify();
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &profile_select,
                window,
                |this, _, event: &ComboboxEvent<SearchableVec<String>>, window, cx| {
                    if let ComboboxEvent::Confirm(values) = event
                        && let Some(profile) = values.first()
                        && profile_selection_changed(
                            this.application.read(cx).state.selected_profile.as_deref(),
                            profile,
                        )
                    {
                        this.select_profile(profile.clone(), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &version_select,
                window,
                |this, _, event: &SelectEvent<Vec<ObjectVersion>>, _, cx| {
                    let SelectEvent::Confirm(version_id) = event;
                    this.application.update(cx, |application, cx| {
                        application.select_version_id(version_id.clone(), cx);
                    });
                },
            ),
            cx.subscribe_in(
                &bucket_list,
                window,
                |this, list, event: &ListEvent, window, cx| {
                    if let ListEvent::Confirm(index) = event {
                        let bucket = list.read(cx).delegate().bucket_at(index.row).cloned();
                        if let Some(bucket) = bucket {
                            this.confirm_bucket(bucket, window, cx);
                        }
                    }
                },
            ),
            cx.subscribe_in(
                &bucket_name_input,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_manual_bucket(value, cx);
                        });
                    }
                    InputEvent::PressEnter {
                        secondary: false, ..
                    } => {
                        this.start_recent_objects_load(false, window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &object_key_input,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_object_key(value, cx);
                        });
                        this.clear_version_select(window, cx);
                    }
                    InputEvent::PressEnter {
                        secondary: false, ..
                    } => {
                        this.start_version_load(window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &destination_input,
                window,
                |this, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_destination(value, cx);
                        });
                    }
                },
            ),
        ];

        let mut this = Self {
            application,
            profile_select,
            version_select,
            bucket_list,
            resizable_state,
            bucket_name_input,
            object_key_input,
            destination_input,
            commands,
            _subscriptions: subscriptions,
        };
        this.sync_bucket_list(window, cx);
        this
    }

    #[cfg(test)]
    fn new_for_test(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: AppState,
        commands: AwsCommands,
    ) -> Self {
        Self::new_with_dependencies(
            window,
            cx,
            Ok(state.profiles.clone()),
            Ok(()),
            commands,
            Some(state),
        )
    }

    fn sync_bucket_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.application.read(cx).state.clone();
        self.bucket_list.update(cx, |list, cx| {
            let delegate = list.delegate_mut();
            delegate.update_from_state(&state);
            let selected_index = state.selected_bucket.as_ref().and_then(|bucket| {
                delegate
                    .visible_buckets
                    .iter()
                    .position(|visible| visible == bucket)
                    .map(IndexPath::new)
            });
            list.set_selected_index(selected_index, window, cx);
            cx.notify();
        });
    }

    fn sync_profile_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self
            .application
            .read(cx)
            .state
            .selected_profile
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        self.profile_select.update(cx, |picker, cx| {
            picker.set_selected_values(&selected, window, cx);
            cx.notify();
        });
    }

    fn select_profile(&mut self, profile: String, window: &mut Window, cx: &mut Context<Self>) {
        if !profile_selection_changed(
            self.application.read(cx).state.selected_profile.as_deref(),
            &profile,
        ) {
            return;
        }
        let selected = self.application.update(cx, |application, cx| {
            application.select_profile(profile.clone(), cx)
        });
        if !selected {
            return;
        }

        self.object_key_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bucket_name_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bucket_list.update(cx, |list, cx| {
            list.set_query("", window, cx);
        });
        self.sync_profile_picker(window, cx);
        self.sync_bucket_list(window, cx);
        self.clear_version_select(window, cx);

        let requested_profile = profile.clone();
        let result_profile = profile;
        self.spawn_bucket_load(requested_profile, result_profile, window, cx);
    }

    fn confirm_bucket(&mut self, bucket: String, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.application.read(cx).state.clone();
        if !bucket_confirmation_enabled(state.loading_buckets, state.downloading) {
            return;
        }

        let changed = state.selected_bucket.as_deref() != Some(bucket.as_str());
        let selected = self
            .application
            .update(cx, |application, cx| application.select_bucket(bucket, cx));
        if !selected {
            return;
        }

        self.sync_bucket_list(window, cx);
        if changed {
            self.clear_version_select(window, cx);
            self.start_recent_objects_load(false, window, cx);
        }
    }

    fn refresh_buckets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = match self
            .application
            .update(cx, |application, cx| application.begin_bucket_refresh(cx))
        {
            Ok(profile) => profile,
            Err(_) => return,
        };

        self.sync_bucket_list(window, cx);
        self.spawn_bucket_load(profile.clone(), profile, window, cx);
    }

    fn start_recent_objects_load(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.application.update(cx, |application, cx| {
            application.begin_recent_objects_load(force, cx)
        }) else {
            return;
        };

        let application = self.application.clone();
        let list_recent_objects = self.commands.list_recent_objects.clone();
        let request_for_cli = request.clone();
        window
            .spawn(cx, async move |cx| {
                let RecentObjectsLoadRequest {
                    profile,
                    bucket,
                    cancelled,
                    ..
                } = request_for_cli;
                let result = cx
                    .background_executor()
                    .spawn(async move { list_recent_objects(&profile, &bucket, cancelled) })
                    .await;

                let result = match result {
                    Ok(Some(objects)) => Ok(objects),
                    Ok(None) => return,
                    Err(error) => Err(error),
                };
                let _ = cx.update(|_, cx| {
                    application.update(cx, |application, cx| {
                        application.finish_recent_objects_load(&request, result, cx);
                    });
                });
            })
            .detach();
    }

    fn cancel_recent_objects_load(&mut self, cx: &mut Context<Self>) {
        self.application.update(cx, |application, cx| {
            application.cancel_recent_objects_load(cx);
        });
    }

    fn select_recent_object(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        self.application.update(cx, |application, cx| {
            application.select_recent_object_key(key.clone(), cx);
        });
        self.object_key_input.update(cx, |input, cx| {
            input.set_value(key, window, cx);
            input.focus(window, cx);
        });
        self.clear_version_select(window, cx);
    }

    fn spawn_bucket_load(
        &self,
        requested_profile: String,
        result_profile: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let application = self.application.clone();
        let version_select = self.version_select.clone();
        let list_buckets = self.commands.list_buckets.clone();
        let bucket_list = self.bucket_list.clone();
        let previous_bucket = self.application.read(cx).state.selected_bucket.clone();
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { list_buckets(&requested_profile) })
                    .await;

                let _ = cx.update(|window, cx| {
                    let accepted = application.update(cx, |application, cx| {
                        application.finish_bucket_load(&result_profile, result, cx)
                    });

                    if accepted {
                        let state = application.read(cx).state.clone();
                        bucket_list.update(cx, |list, cx| {
                            let delegate = list.delegate_mut();
                            delegate.update_from_state(&state);
                            let selected_index =
                                state.selected_bucket.as_ref().and_then(|bucket| {
                                    delegate
                                        .visible_buckets
                                        .iter()
                                        .position(|visible| visible == bucket)
                                        .map(IndexPath::new)
                                });
                            list.set_selected_index(selected_index, window, cx);
                            cx.notify();
                        });
                    }

                    if accepted && application.read(cx).state.selected_bucket != previous_bucket {
                        version_select.update(cx, |select, cx| {
                            select.set_items(Vec::new(), window, cx);
                            select.set_selected_index(None, window, cx);
                            cx.notify();
                        });
                    }
                });
            })
            .detach();
    }

    fn start_version_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = match self
            .application
            .update(cx, |application, cx| application.begin_version_load(cx))
        {
            Ok(request) => request,
            Err(_) => return,
        };

        let application = self.application.clone();
        let version_select = self.version_select.clone();
        let list_versions = self.commands.list_versions.clone();
        let request_for_cli = request.clone();
        window
            .spawn(cx, async move |cx| {
                let VersionLoadRequest {
                    profile,
                    bucket,
                    key,
                    ..
                } = request_for_cli;
                let result = cx
                    .background_executor()
                    .spawn(async move { list_versions(&profile, &bucket, &key) })
                    .await;
                let versions = result.clone().unwrap_or_default();

                let _ = cx.update(|window, cx| {
                    let accepted = application.update(cx, |application, cx| {
                        application.finish_version_load(&request, result, cx)
                    });

                    if accepted {
                        version_select.update(cx, |select, cx| {
                            select.set_items(versions, window, cx);
                            select.set_selected_index(None, window, cx);
                            cx.notify();
                        });
                    }
                });
            })
            .detach();
    }

    fn clear_version_select(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.version_select.update(cx, |select, cx| {
            select.set_items(Vec::new(), window, cx);
            select.set_selected_index(None, window, cx);
            cx.notify();
        });
    }

    fn choose_destination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.application.read(cx).state.clone();
        let directory = destination_directory(&state.destination);
        let suggested_name = state.suggested_filename();
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested_name));
        let application = self.application.clone();
        let destination_input = self.destination_input.clone();

        window
            .spawn(cx, async move |cx| match receiver.await {
                Ok(Ok(Some(path))) => {
                    let value = path.display().to_string();
                    let _ = cx.update(|window, cx| {
                        application.update(cx, |application, cx| {
                            application.set_destination(value.clone(), cx);
                        });
                        destination_input.update(cx, |input, cx| {
                            input.set_value(value, window, cx);
                        });
                    });
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => set_application_error(&application, error.to_string(), cx).await,
                Err(error) => {
                    set_application_error(&application, error.to_string(), cx).await;
                }
            })
            .detach();
    }

    fn start_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = match self
            .application
            .update(cx, |application, cx| application.begin_download(cx))
        {
            Ok(request) => request,
            Err(_) => return,
        };

        let DownloadRequest {
            profile,
            bucket,
            key,
            version_id,
            destination,
        } = request;
        let destination_display = destination.display().to_string();
        let application = self.application.clone();
        let download = self.commands.download.clone();

        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        download(&profile, &bucket, &key, version_id.as_deref(), &destination)
                    })
                    .await;

                let _ = cx.update(|_, cx| {
                    application.update(cx, |application, cx| {
                        application.finish_download(destination_display, result, cx);
                    });
                });
            })
            .detach();
    }

    fn render_sidebar(&self, state: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_foreground = cx.theme().muted_foreground;
        let profile_select = Combobox::new(&self.profile_select)
            .placeholder("Select an AWS profile")
            .search_placeholder("Search profiles")
            .empty(|_, cx| {
                div()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No matching AWS profiles")
            })
            .disabled(state.profiles.is_empty())
            .w_full();
        let profile_picker = div().id("profile-picker").child(profile_select);
        #[cfg(test)]
        let profile_picker = profile_picker.test_support();
        let refresh_disabled =
            state.selected_profile.is_none() || state.loading_buckets || state.downloading;
        let refresh = Button::new("refresh-buckets")
            .ghost()
            .small()
            .label("Refresh")
            .loading(state.loading_buckets && !state.buckets.is_empty())
            .disabled(refresh_disabled)
            .on_click(cx.listener(|this, _, window, cx| {
                this.refresh_buckets(window, cx);
            }));

        let footer_label = if state.manual_bucket_entry {
            "Manual bucket entry".to_string()
        } else {
            format!("{} buckets loaded", state.buckets.len())
        };

        let selected_bucket_hidden = self
            .bucket_list
            .read(cx)
            .delegate()
            .selected_bucket_is_hidden();
        let header = v_flex()
            .id("sidebar-header")
            .gap_3()
            .w_full()
            .p_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(IconName::Folder).size_4())
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().font_semibold().child("S3 Downloader"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted_foreground)
                                    .child("AWS object downloads"),
                            ),
                    ),
            )
            .child(div().text_xs().font_medium().child("AWS profile"))
            .child(profile_picker)
            .child(div().text_xs().text_color(muted_foreground).child(
                if state.selected_profile.is_some() {
                    "AWS CLI profile active"
                } else {
                    "Choose a profile to load buckets"
                },
            ))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .child(format!("Buckets · {}", state.buckets.len())),
                    )
                    .child(refresh),
            )
            .when(state.manual_bucket_entry, |this| {
                this.child(div().text_xs().font_medium().child("Bucket name"))
                    .child(
                        Input::new(&self.bucket_name_input)
                            .cleanable(true)
                            .disabled(state.loading_buckets || state.downloading)
                            .w_full(),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted_foreground)
                            .child("Enter a bucket name, then press Enter to scan it."),
                    )
            })
            .when(selected_bucket_hidden, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(muted_foreground)
                        .child("Selected bucket is hidden by the filter"),
                )
            });
        #[cfg(test)]
        let header = header.test_support();
        let bucket_list = List::new(&self.bucket_list)
            .search_placeholder("Filter buckets")
            .w_full()
            .h_full();
        let footer = div()
            .id("sidebar-footer")
            .w_full()
            .p_3()
            .text_xs()
            .text_color(muted_foreground)
            .child(footer_label);
        #[cfg(test)]
        let footer = footer.test_support();

        let sidebar = v_flex()
            .id("sidebar-content")
            .h_full()
            .w_full()
            .overflow_hidden()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(bucket_list),
            )
            .child(footer);
        #[cfg(test)]
        let sidebar = sidebar.test_support();
        sidebar
    }

    fn render_recent_objects(&self, state: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_identity = state
            .selected_profile
            .as_deref()
            .zip(state.selected_bucket.as_deref());
        let matches_selection = selected_identity
            .is_some_and(|(profile, bucket)| state.recent_objects.matches(profile, bucket));

        let action = if matches_selection {
            match &state.recent_objects {
                RecentObjectsState::Loading { .. } => Button::new("cancel-recent-objects")
                    .ghost()
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_recent_objects_load(cx);
                    }))
                    .into_any_element(),
                RecentObjectsState::Loaded { .. } => Button::new("refresh-recent-objects")
                    .ghost()
                    .small()
                    .label("Refresh")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_recent_objects_load(true, window, cx);
                    }))
                    .into_any_element(),
                RecentObjectsState::Failed { .. } | RecentObjectsState::Cancelled { .. } => {
                    Button::new("retry-recent-objects")
                        .ghost()
                        .small()
                        .label("Retry")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_recent_objects_load(true, window, cx);
                        }))
                        .into_any_element()
                }
                RecentObjectsState::Idle => div().into_any_element(),
            }
        } else {
            div().into_any_element()
        };

        let content = if !matches_selection {
            let message = match (
                state.selected_profile.is_some(),
                state.selected_bucket.is_some(),
                state.manual_bucket_entry,
            ) {
                (false, _, _) => "Select an AWS profile to see recent objects.",
                (_, false, true) => "Enter a bucket name and press Enter to scan it.",
                (_, true, true) => "Press Enter in the bucket name field to scan this bucket.",
                (_, false, false) => "Select a bucket to see recent objects.",
                (_, true, false) => "Recent objects will appear here.",
            };
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(message)
                .into_any_element()
        } else {
            match &state.recent_objects {
                RecentObjectsState::Idle => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Recent objects will appear here.")
                    .into_any_element(),
                RecentObjectsState::Loading { .. } => h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(Spinner::new().xsmall())
                    .child("Scanning bucket for recent objects…")
                    .into_any_element(),
                RecentObjectsState::Loaded { objects, .. } if objects.is_empty() => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No objects found")
                    .into_any_element(),
                RecentObjectsState::Loaded { objects, .. } => v_flex()
                    .gap_1()
                    .children(objects.iter().map(|object| {
                        let profile = state.selected_profile.as_deref().unwrap_or_default();
                        let bucket = state.selected_bucket.as_deref().unwrap_or_default();
                        let timestamp = object.last_modified.format("%Y-%m-%d %H:%M:%S UTC");
                        let key = object.key.clone();
                        let identity = format!(
                            "recent-object:{}:{}:{}:{}:{}",
                            profile.len(),
                            profile,
                            bucket.len(),
                            bucket,
                            key
                        );
                        h_flex()
                            .gap_2()
                            .items_center()
                            .w_full()
                            .child(
                                Button::new(ElementId::Name(identity.into()))
                                    .ghost()
                                    .small()
                                    .flex_1()
                                    .disabled(state.downloading || state.loading_versions)
                                    .tooltip(key.clone())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_recent_object(key.clone(), window, cx);
                                    }))
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .w_full()
                                            .min_w_0()
                                            .child(Icon::new(IconName::File).size_4())
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_x_hidden()
                                                    .text_ellipsis()
                                                    .child(object.key.clone()),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(timestamp.to_string()),
                            )
                    }))
                    .into_any_element(),
                RecentObjectsState::Failed { error, .. } => {
                    let message = if aws::cli::is_access_denied_error(error) {
                        format!(
                            "Could not list recent objects: {error}. Confirm this profile has s3:ListBucket permission."
                        )
                    } else {
                        format!("Could not list recent objects: {error}")
                    };
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(message)
                        .into_any_element()
                }
                RecentObjectsState::Cancelled { .. } => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Recent object scan cancelled.")
                    .into_any_element(),
            }
        };

        v_flex()
            .gap_2()
            .w_full()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_medium().child("Recent objects"))
                    .child(action),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Newest by modification time · UTC"),
            )
            .child(content)
    }

    fn resize_sidebar_by(&mut self, change_rems: f32, window: &mut Window, cx: &mut Context<Self>) {
        let width_rem = window.bounds().size.width / window.rem_size();
        let max_rem = sidebar_width_max_rem(width_rem);
        let current_rem = self
            .resizable_state
            .read(cx)
            .sizes()
            .first()
            .copied()
            .map(|size| size / window.rem_size())
            .unwrap_or(SIDEBAR_WIDTH_REMS);
        let target_rem = (current_rem + change_rems).clamp(SIDEBAR_MIN_WIDTH_REMS, max_rem);
        let target_width = rems(target_rem).to_pixels(window.rem_size());
        self.resizable_state.update(cx, |state, cx| {
            state.resize_panel(0, target_width, window, cx);
        });
    }

    fn reset_sidebar_width(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let width_rem = window.bounds().size.width / window.rem_size();
        let target_rem = SIDEBAR_WIDTH_REMS.min(sidebar_width_max_rem(width_rem));
        let target_width = rems(target_rem).to_pixels(window.rem_size());
        self.resizable_state.update(cx, |state, cx| {
            state.resize_panel(0, target_width, window, cx);
        });
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.application.read(cx).state.clone();
        let has_bucket = state.selected_bucket.is_some();
        let form_disabled = !has_bucket || state.loading_buckets || state.downloading;
        let version_disabled = form_disabled || state.loading_versions || state.versions.is_empty();
        let version_load_disabled =
            form_disabled || state.loading_versions || state.object_key.trim().is_empty();
        let download_disabled = form_disabled
            || state.loading_versions
            || state.object_key.trim().is_empty()
            || state.destination.trim().is_empty();

        let version_control = h_flex()
            .gap_2()
            .w_full()
            .child(
                Select::new(&self.version_select)
                    .placeholder("Current version")
                    .cleanable(true)
                    .disabled(version_disabled)
                    .flex_1(),
            )
            .child(
                Button::new("load-versions")
                    .outline()
                    .small()
                    .label("Load versions")
                    .loading(state.loading_versions)
                    .disabled(version_load_disabled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_version_load(window, cx);
                    })),
            )
            .into_any_element();

        let context = match (&state.selected_bucket, &state.selected_profile) {
            (Some(bucket), Some(profile)) => h_flex()
                .gap_2()
                .w_full()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .p_3()
                .child(Icon::new(IconName::Folder).size_4())
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .overflow_x_hidden()
                                .text_ellipsis()
                                .text_sm()
                                .font_medium()
                                .child(bucket.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("AWS profile: {profile}")),
                        ),
                )
                .into_any_element(),
            _ => div()
                .w_full()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Choose an AWS profile and bucket to begin.")
                .into_any_element(),
        };

        let version_summary = state
            .selected_version_id
            .as_deref()
            .map(|version| format!("Selected version: {version}"))
            .unwrap_or_else(|| "Current version".to_string());
        let file_summary = h_flex()
            .gap_2()
            .w_full()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::File).size_4())
            .child(
                div()
                    .flex_1()
                    .overflow_x_hidden()
                    .text_ellipsis()
                    .child(format!(
                        "File: {} · {}",
                        state.suggested_filename(),
                        version_summary
                    )),
            );
        let status = status_element(state.status.clone());
        let readiness = readiness_for(&state);
        let content = v_flex()
            .gap_6()
            .p_6()
            .w_full()
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_xl().font_semibold().child("Download object"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Choose an object and save it to your Mac."),
                    ),
            )
            .child(
                v_flex().gap_4().w_full().child(context).child(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .child(section_heading(
                            "Object",
                            "Identify the S3 object to download.",
                            cx.theme().muted_foreground,
                        ))
                        .child(field_row(
                            "Object key",
                            Input::new(&self.object_key_input)
                                .disabled(form_disabled)
                                .w_full(),
                            "Enter the full key, including folders.",
                            false,
                            cx.theme().muted_foreground,
                        ))
                        .child(
                            h_flex()
                                .gap_4()
                                .w_full()
                                .child(div().w_24().flex_shrink_0())
                                .child(self.render_recent_objects(&state, cx)),
                        )
                        .child(field_row(
                            "Version",
                            version_control,
                            "Optional — leave blank to download the current version.",
                            true,
                            cx.theme().muted_foreground,
                        ))
                        .child(field_row(
                            "Save to",
                            h_flex()
                                .gap_2()
                                .w_full()
                                .child(
                                    Input::new(&self.destination_input)
                                        .disabled(form_disabled)
                                        .flex_1(),
                                )
                                .child(
                                    Button::new("choose-destination")
                                        .outline()
                                        .small()
                                        .label("Choose…")
                                        .disabled(form_disabled)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_destination(window, cx);
                                        })),
                                ),
                            "Choose the local file path.",
                            false,
                            cx.theme().muted_foreground,
                        ))
                        .child(file_summary),
                ),
            )
            .child(
                v_flex().gap_3().w_full().child(status).child(
                    h_flex()
                        .gap_4()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(readiness),
                        )
                        .child(
                            Button::new("download")
                                .primary()
                                .label("Download")
                                .loading(state.downloading)
                                .disabled(download_disabled)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_download(window, cx);
                                })),
                        ),
                ),
            );

        let rem_size = window.rem_size();
        let total_width_rem = window.bounds().size.width / rem_size;
        let sidebar_max_rem = sidebar_width_max_rem(total_width_rem);
        let sidebar_min = rems(SIDEBAR_MIN_WIDTH_REMS).to_pixels(rem_size);
        let sidebar_max = rems(sidebar_max_rem).to_pixels(rem_size);
        let sidebar_default = rems(SIDEBAR_WIDTH_REMS.min(sidebar_max_rem)).to_pixels(rem_size);
        let workspace_min = rems(WORKSPACE_MIN_WIDTH_REMS).to_pixels(rem_size);

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context("MainWindow")
            .on_action(cx.listener(|this, _: &NarrowSidebar, window, cx| {
                this.resize_sidebar_by(-1.0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &WidenSidebar, window, cx| {
                this.resize_sidebar_by(1.0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ResetSidebar, window, cx| {
                this.reset_sidebar_width(window, cx);
            }))
            .child(
                h_resizable("s3-downloader-panels")
                    .with_state(&self.resizable_state)
                    .child(
                        resizable_panel()
                            .size(sidebar_default)
                            .size_range(sidebar_min..sidebar_max)
                            .flex_none()
                            .child(self.render_sidebar(&state, cx)),
                    )
                    .child(
                        resizable_panel()
                            .size_range(workspace_min..gpui_kit::Pixels::MAX)
                            .child(
                                v_flex().size_full().overflow_hidden().child(
                                    v_flex().size_full().overflow_y_scrollbar().child(
                                        v_flex()
                                            .w_full()
                                            .max_w(rems(CONTENT_MAX_WIDTH_REMS))
                                            .child(content),
                                    ),
                                ),
                            ),
                    ),
            )
    }
}

fn filter_buckets(buckets: &[String], query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return buckets.to_vec();
    }

    buckets
        .iter()
        .filter(|bucket| bucket.to_lowercase().contains(&query))
        .cloned()
        .collect()
}

fn readiness_for(state: &AppState) -> &'static str {
    if state.downloading {
        "Downloading…"
    } else if state.loading_buckets {
        "Loading buckets…"
    } else if state.loading_versions {
        "Loading versions…"
    } else if state.selected_profile.is_none() {
        "Choose an AWS profile"
    } else if state.selected_bucket.is_none() {
        "Choose a bucket"
    } else if state.object_key.trim().is_empty() {
        "Enter an object key"
    } else if state.destination.trim().is_empty() {
        "Choose where to save the file"
    } else {
        "Ready to download"
    }
}

fn section_heading(
    title: &'static str,
    hint: &'static str,
    muted_foreground: gpui_kit::Hsla,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .w_full()
        .child(div().text_lg().font_semibold().child(title))
        .child(div().text_sm().text_color(muted_foreground).child(hint))
}

fn field_row(
    label: &'static str,
    control: impl IntoElement,
    hint: &'static str,
    optional: bool,
    muted_foreground: gpui_kit::Hsla,
) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap_4()
        .w_full()
        .child(
            v_flex()
                .gap_1()
                .w_24()
                .flex_shrink_0()
                .pt_2()
                .child(div().text_sm().font_medium().child(label))
                .child(if optional {
                    div()
                        .text_xs()
                        .text_color(muted_foreground)
                        .child("Optional")
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        )
        .child(
            v_flex()
                .gap_2()
                .flex_1()
                .child(control)
                .child(div().text_xs().text_color(muted_foreground).child(hint)),
        )
}

fn status_element(status: Option<AppStatus>) -> AnyElement {
    match status {
        Some(AppStatus::Info(_)) => div().into_any_element(),
        Some(AppStatus::Success(message)) => {
            Alert::success("status-success", message).into_any_element()
        }
        Some(AppStatus::Error(message)) => Alert::error("status-error", message).into_any_element(),
        None => div().into_any_element(),
    }
}

fn format_size(size: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let size = size as f64;
    if size < KIB {
        format!("{size:.0} B")
    } else if size < MIB {
        format!("{:.1} KiB", size / KIB)
    } else if size < GIB {
        format!("{:.1} MiB", size / MIB)
    } else {
        format!("{:.1} GiB", size / GIB)
    }
}

fn destination_directory(destination: &str) -> PathBuf {
    if destination.trim().is_empty() {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }

    Path::new(destination.trim())
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn set_application_error(
    application: &Entity<DownloaderApp>,
    error: String,
    cx: &mut gpui_kit::AsyncWindowContext,
) {
    let _ = cx.update(|_, cx| {
        application.update(cx, |application, cx| {
            application.set_error(error, cx);
        });
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        AwsCommands, MainWindow, bucket_confirmation_enabled, filter_buckets,
        profile_selection_changed, readiness_for, sidebar_width_max_rem,
    };
    use crate::models::AppState;
    use crate::models::{ObjectVersion, RecentObject, RecentObjectsState};
    use crate::{NarrowSidebar, ResetSidebar, WidenSidebar};
    use gpui_kit::{
        AppContext, Bounds, Entity, Focusable, KeyBinding, ScrollDelta, TestAppContext,
        WindowBounds, WindowOptions,
        component::theme::{Theme, ThemeMode},
        point, px, size,
        test::{TestAppContextExt, TestWindowExt},
    };

    fn fixture_state(selected_profile: Option<&str>, buckets: &[&str]) -> AppState {
        AppState {
            profiles: vec!["default".to_string(), "production".to_string()],
            selected_profile: selected_profile.map(str::to_string),
            buckets: buckets.iter().map(|bucket| (*bucket).to_string()).collect(),
            ..Default::default()
        }
    }

    fn fixture_commands(
        bucket_loads: std::sync::Arc<AtomicUsize>,
        recent_loads: std::sync::Arc<AtomicUsize>,
    ) -> AwsCommands {
        AwsCommands {
            list_buckets: std::sync::Arc::new(move |_| {
                bucket_loads.fetch_add(1, Ordering::SeqCst);
                Ok(vec!["alpha-bucket".to_string(), "beta-bucket".to_string()])
            }),
            list_recent_objects: std::sync::Arc::new(move |_, _, _| {
                recent_loads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(Vec::new()))
            }),
            list_versions: std::sync::Arc::new(|_, _, _| Ok(Vec::new())),
            download: std::sync::Arc::new(|_, _, _, _, _| Ok(())),
        }
    }

    fn open_main_window(
        cx: &mut TestAppContext,
        state: AppState,
        commands: AwsCommands,
        width: f32,
        height: f32,
    ) -> (gpui_kit::AnyWindowHandle, Entity<MainWindow>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("cmd-alt-left", NarrowSidebar, Some("MainWindow")),
                KeyBinding::new("cmd-alt-right", WidenSidebar, Some("MainWindow")),
                KeyBinding::new("cmd-alt-0", ResetSidebar, Some("MainWindow")),
            ]);
        });

        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(width), px(height)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| MainWindow::new_for_test(window, cx, state, commands)),
            )
            .expect("open MainWindow fixture")
        })
    }

    fn run_until_parked(cx: &mut TestAppContext) {
        cx.run_until_parked();
    }

    #[test]
    fn bucket_filter_preserves_order_for_an_empty_query() {
        let buckets = vec!["zeta".to_string(), "alpha".to_string(), "beta".to_string()];

        assert_eq!(filter_buckets(&buckets, ""), buckets);
        assert_eq!(filter_buckets(&buckets, "   "), buckets);
    }

    #[test]
    fn bucket_filter_trims_and_matches_case_insensitively() {
        let buckets = vec![
            "client-production".to_string(),
            "client-staging".to_string(),
            "archive".to_string(),
        ];

        assert_eq!(
            filter_buckets(&buckets, "  PRODUCTION "),
            vec!["client-production".to_string()]
        );
        assert_eq!(
            filter_buckets(&buckets, "client"),
            vec![
                "client-production".to_string(),
                "client-staging".to_string()
            ]
        );
    }

    #[test]
    fn bucket_filter_reports_no_matches_without_mutating_input() {
        let buckets = vec!["alpha".to_string(), "beta".to_string()];

        assert!(filter_buckets(&buckets, "missing").is_empty());
        assert_eq!(buckets, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn readiness_describes_the_first_missing_requirement() {
        let mut state = AppState::default();
        assert_eq!(readiness_for(&state), "Choose an AWS profile");

        state.selected_profile = Some("default".to_string());
        assert_eq!(readiness_for(&state), "Choose a bucket");

        state.selected_bucket = Some("client-production".to_string());
        assert_eq!(readiness_for(&state), "Enter an object key");

        state.object_key = "folder/object.txt".to_string();
        assert_eq!(readiness_for(&state), "Choose where to save the file");

        state.destination = "/tmp/object.txt".to_string();
        assert_eq!(readiness_for(&state), "Ready to download");
    }

    #[test]
    fn profile_selection_only_commits_a_different_value() {
        assert!(!profile_selection_changed(Some("staging"), "staging"));
        assert!(profile_selection_changed(Some("staging"), "production"));
        assert!(profile_selection_changed(None, "default"));
    }

    #[test]
    fn bucket_confirmation_is_blocked_during_conflicting_work() {
        assert!(bucket_confirmation_enabled(false, false));
        assert!(!bucket_confirmation_enabled(true, false));
        assert!(!bucket_confirmation_enabled(false, true));
        assert!(!bucket_confirmation_enabled(true, true));
    }

    #[test]
    fn sidebar_maximum_reserves_the_minimum_workspace_width() {
        assert_eq!(sidebar_width_max_rem(60.0), 28.0);
        assert_eq!(sidebar_width_max_rem(56.25), 26.25);
    }

    #[gpui_kit::test]
    async fn profile_picker_searches_locally_and_commits_only_a_changed_profile(
        cx: &mut TestAppContext,
    ) {
        let bucket_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let recent_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let commands = fixture_commands(bucket_loads.clone(), recent_loads.clone());
        let (handle, main_window) =
            open_main_window(cx, fixture_state(None, &[]), commands, 960.0, 640.0);

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let profile_picker = main_window.read(cx).profile_select.entity_id();
            window
                .within("profile-picker")
                .click(("multi-combo-box", profile_picker), cx);
            window.input("production", cx);
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(bucket_loads.load(Ordering::SeqCst), 0);
        assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
        cx.update(|cx| {
            assert_eq!(
                main_window
                    .read(cx)
                    .profile_select
                    .read(cx)
                    .query(cx)
                    .as_ref(),
                "production"
            );
            assert_eq!(
                main_window
                    .read(cx)
                    .application
                    .read(cx)
                    .state
                    .selected_profile,
                None
            );
        });

        cx.update_window(handle, |_, window, cx| {
            window.press("down", cx);
            window.press("enter", cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, _| {
            bucket_loads.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(bucket_loads.load(Ordering::SeqCst), 1);
        cx.update(|cx| {
            assert_eq!(
                main_window
                    .read(cx)
                    .application
                    .read(cx)
                    .state
                    .selected_profile
                    .as_deref(),
                Some("production")
            );
        });

        // Dismissing a searchable picker emits its active value; it must not
        // make a duplicate bucket request or lose focus restoration.
        cx.update_window(handle, |_, window, cx| {
            let profile_picker = main_window.read(cx).profile_select.entity_id();
            window
                .within("profile-picker")
                .click(("multi-combo-box", profile_picker), cx);
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(
                main_window
                    .read(cx)
                    .profile_select
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .unwrap();
        cx.update(|cx| {
            main_window.update(cx, |this, cx| {
                this.profile_select.update(cx, |_, cx| {
                    cx.emit(super::ComboboxEvent::Confirm(vec![
                        "production".to_string(),
                    ]));
                });
            });
        });
        run_until_parked(cx);
        assert_eq!(bucket_loads.load(Ordering::SeqCst), 1);
        assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
    }

    #[gpui_kit::test]
    async fn bucket_keyboard_highlights_until_confirmation_and_suppresses_duplicate_scans(
        cx: &mut TestAppContext,
    ) {
        let bucket_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let recent_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let commands = fixture_commands(bucket_loads, recent_loads.clone());
        let (handle, main_window) = open_main_window(
            cx,
            fixture_state(Some("default"), &["alpha-bucket", "beta-bucket"]),
            commands,
            960.0,
            640.0,
        );

        cx.update_window(handle, |_, window, cx| {
            let bucket_list = main_window.read(cx).bucket_list.clone();
            bucket_list.update(cx, |list, cx| list.focus(window, cx));
            window.render_frame(cx);
            window.press("down", cx);
            assert_eq!(
                main_window.read(cx).bucket_list.read(cx).selected_index(),
                Some(super::IndexPath::new(0))
            );
            assert_eq!(
                main_window
                    .read(cx)
                    .application
                    .read(cx)
                    .state
                    .selected_bucket,
                None
            );
            assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
            window.press("enter", cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, _| {
            recent_loads.load(Ordering::SeqCst) == 1
        })
        .await;
        cx.update_window(handle, |_, window, cx| {
            window.press("enter", cx);
            window.render_frame(cx);
        })
        .unwrap();
        run_until_parked(cx);
        assert_eq!(recent_loads.load(Ordering::SeqCst), 1);

        cx.update_window(handle, |_, window, cx| {
            window.click("bucket-label:default:beta-bucket", cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, _| {
            recent_loads.load(Ordering::SeqCst) == 2
        })
        .await;
        cx.update_window(handle, |_, window, cx| {
            window.click("bucket-label:default:beta-bucket", cx);
            window.render_frame(cx);
        })
        .unwrap();
        run_until_parked(cx);
        assert_eq!(recent_loads.load(Ordering::SeqCst), 2);
        cx.update(|cx| {
            assert_eq!(
                main_window
                    .read(cx)
                    .application
                    .read(cx)
                    .state
                    .selected_bucket
                    .as_deref(),
                Some("beta-bucket")
            );
        });
    }

    #[gpui_kit::test]
    async fn bucket_filter_refresh_and_disabled_confirmation_preserve_selection(
        cx: &mut TestAppContext,
    ) {
        let bucket_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let recent_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let commands = fixture_commands(bucket_loads.clone(), recent_loads.clone());
        let long_bucket = "very-long-client-bucket-name-that-does-not-fit-in-sidebar";
        let mut state = fixture_state(
            Some("default"),
            &["alpha-bucket", "beta-bucket", long_bucket],
        );
        state.selected_bucket = Some("beta-bucket".to_string());
        let (handle, main_window) = open_main_window(cx, state, commands, 960.0, 640.0);

        cx.update_window(handle, |_, window, cx| {
            let list = main_window.read(cx).bucket_list.clone();
            list.update(cx, |list, cx| list.set_query(" ALPHA ", window, cx));
            window.render_frame(cx);
            assert!(
                window
                    .try_find("bucket-label:default:alpha-bucket")
                    .is_some()
            );
            assert!(
                window
                    .try_find("bucket-label:default:beta-bucket")
                    .is_none()
            );
            assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
            list.update(cx, |list, cx| list.set_query("", window, cx));
            window.render_frame(cx);
            assert_eq!(
                window
                    .find(format!("bucket-row:default:{long_bucket}"))
                    .label(),
                Some(long_bucket)
            );
            let button = window
                .find(format!("bucket-label:default:{long_bucket}"))
                .bounds();
            let icon = window
                .find(format!("bucket-icon:default:{long_bucket}"))
                .bounds();
            let label = window
                .find(format!("bucket-name:default:{long_bucket}"))
                .bounds();
            assert!(label.left() >= icon.right());
            assert!(
                label.left() - button.left() < super::rems(4.0).to_pixels(window.rem_size()),
                "bucket text should begin near the leading edge, not at the row center"
            );

            main_window.update(cx, |this, cx| {
                this.application.update(cx, |app, cx| {
                    app.state.downloading = true;
                    cx.notify();
                });
                this.confirm_bucket("alpha-bucket".to_string(), window, cx);
            });
            assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
        })
        .unwrap();

        cx.update_window(handle, |_, window, cx| {
            main_window.update(cx, |this, cx| {
                this.application.update(cx, |app, cx| {
                    app.state.downloading = false;
                    cx.notify();
                });
                this.refresh_buckets(window, cx);
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, _| {
            bucket_loads.load(Ordering::SeqCst) == 1
        })
        .await;
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, cx| {
            !main_window
                .read(cx)
                .application
                .read(cx)
                .state
                .loading_buckets
        })
        .await;
        cx.update(|cx| {
            assert_eq!(
                main_window
                    .read(cx)
                    .application
                    .read(cx)
                    .state
                    .selected_bucket
                    .as_deref(),
                Some("beta-bucket")
            );
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                main_window
                    .read(cx)
                    .bucket_list
                    .read(cx)
                    .delegate()
                    .confirmed_bucket
                    .as_deref(),
                Some("beta-bucket")
            );
        })
        .unwrap();
        assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
    }

    #[gpui_kit::test]
    async fn bucket_list_scroll_does_not_move_sidebar_header_or_footer(cx: &mut TestAppContext) {
        let mut state = fixture_state(Some("default"), &[]);
        state.buckets = (0..80)
            .map(|index| format!("scroll-bucket-{index:02}"))
            .collect();
        let (handle, main_window) = open_main_window(
            cx,
            state,
            fixture_commands(
                std::sync::Arc::new(AtomicUsize::new(0)),
                std::sync::Arc::new(AtomicUsize::new(0)),
            ),
            960.0,
            640.0,
        );

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let header_before = window.find("sidebar-header").bounds();
            let footer_before = window.find("sidebar-footer").bounds();
            let list = main_window.read(cx).bucket_list.clone();
            let offset_before = list.read(cx).scroll_handle().offset().y;

            window.scroll(
                "bucket-row:default:scroll-bucket-00",
                ScrollDelta::Lines(point(0.0, -24.0)),
                cx,
            );
            window.render_frame(cx);

            let offset_after = list.read(cx).scroll_handle().offset().y;
            assert_ne!(offset_before, offset_after, "bucket list should scroll");
            assert_eq!(window.find("sidebar-header").bounds(), header_before);
            assert_eq!(window.find("sidebar-footer").bounds(), footer_before);
            assert!(
                window
                    .try_find("bucket-row:default:scroll-bucket-00")
                    .is_none(),
                "the first bucket should leave the list viewport"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    async fn recent_object_selection_updates_and_focuses_key_and_clears_version(
        cx: &mut TestAppContext,
    ) {
        let bucket_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let recent_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let mut state = fixture_state(Some("default"), &["alpha-bucket"]);
        state.selected_bucket = Some("alpha-bucket".to_string());
        state.object_key = "old/key.csv".to_string();
        state.versions = vec![ObjectVersion {
            key: "old/key.csv".to_string(),
            version_id: "version-1".to_string(),
            is_latest: true,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            size: 12,
        }];
        state.selected_version_id = Some("version-1".to_string());
        state.recent_objects = RecentObjectsState::Loaded {
            profile: "default".to_string(),
            bucket: "alpha-bucket".to_string(),
            request_id: 1,
            objects: vec![RecentObject {
                key: "reports/latest.csv".to_string(),
                last_modified: chrono::Utc::now(),
            }],
        };
        let commands = fixture_commands(bucket_loads, recent_loads.clone());
        let (handle, main_window) = open_main_window(cx, state, commands, 960.0, 640.0);

        cx.update_window(handle, |_, window, cx| {
            main_window.update(cx, |this, cx| {
                let version = ObjectVersion {
                    key: "old/key.csv".to_string(),
                    version_id: "version-1".to_string(),
                    is_latest: true,
                    last_modified: "2026-01-01T00:00:00Z".to_string(),
                    size: 12,
                };
                this.version_select.update(cx, |select, cx| {
                    select.set_items(vec![version], window, cx);
                    select.set_selected_index(Some(super::IndexPath::new(0)), window, cx);
                });
            });
            window.render_frame(cx);
            window.click(
                "recent-object:7:default:12:alpha-bucket:reports/latest.csv",
                cx,
            );
            window.render_frame(cx);
            assert!(
                main_window
                    .read(cx)
                    .object_key_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .unwrap();

        cx.update(|cx| {
            let state = &main_window.read(cx).application.read(cx).state;
            assert_eq!(state.object_key, "reports/latest.csv");
            assert!(state.versions.is_empty());
            assert_eq!(state.selected_version_id, None);
            assert_eq!(
                main_window.read(cx).object_key_input.read(cx).value(),
                "reports/latest.csv"
            );
            assert_eq!(
                main_window
                    .read(cx)
                    .version_select
                    .read(cx)
                    .selected_index(cx),
                None
            );
        });
        assert_eq!(recent_loads.load(Ordering::SeqCst), 0);
    }

    #[gpui_kit::test]
    async fn changing_buckets_cancels_old_scan_and_completion_preserves_typed_key(
        cx: &mut TestAppContext,
    ) {
        let bucket_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let recent_loads = std::sync::Arc::new(AtomicUsize::new(0));
        let cancellation_tokens = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls = recent_loads.clone();
        let observed_tokens = cancellation_tokens.clone();
        let commands = AwsCommands {
            list_buckets: std::sync::Arc::new(move |_| {
                bucket_loads.fetch_add(1, Ordering::SeqCst);
                Ok(vec!["alpha-bucket".to_string(), "beta-bucket".to_string()])
            }),
            list_recent_objects: std::sync::Arc::new(move |_, bucket, cancelled| {
                calls.fetch_add(1, Ordering::SeqCst);
                observed_tokens
                    .lock()
                    .unwrap()
                    .push((bucket.to_string(), cancelled.clone()));
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    return Ok(None);
                }
                Ok(Some(vec![RecentObject {
                    key: format!("{bucket}/fresh.txt"),
                    last_modified: chrono::Utc::now(),
                }]))
            }),
            list_versions: std::sync::Arc::new(|_, _, _| Ok(Vec::new())),
            download: std::sync::Arc::new(|_, _, _, _, _| Ok(())),
        };
        let (handle, main_window) = open_main_window(
            cx,
            fixture_state(Some("default"), &["alpha-bucket", "beta-bucket"]),
            commands,
            960.0,
            640.0,
        );

        cx.update_window(handle, |_, window, cx| {
            main_window.update(cx, |this, cx| {
                this.confirm_bucket("alpha-bucket".to_string(), window, cx);
                this.confirm_bucket("beta-bucket".to_string(), window, cx);
                this.application.update(cx, |application, cx| {
                    application.set_object_key("typed/during-scan.txt".to_string(), cx);
                });
                this.object_key_input.update(cx, |input, cx| {
                    input.set_value("typed/during-scan.txt", window, cx);
                });
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.wait_for(handle, std::time::Duration::from_secs(1), |_, cx| {
            matches!(
                &main_window.read(cx).application.read(cx).state.recent_objects,
                RecentObjectsState::Loaded { bucket, .. } if bucket == "beta-bucket"
            )
        })
        .await;

        cx.update(|cx| {
            let state = &main_window.read(cx).application.read(cx).state;
            assert_eq!(state.selected_bucket.as_deref(), Some("beta-bucket"));
            assert_eq!(state.object_key, "typed/during-scan.txt");
            assert_eq!(
                main_window.read(cx).object_key_input.read(cx).value(),
                "typed/during-scan.txt"
            );
            assert!(matches!(
                &state.recent_objects,
                RecentObjectsState::Loaded { objects, .. }
                    if objects.first().is_some_and(|object| object.key == "beta-bucket/fresh.txt")
            ));
        });
        assert_eq!(recent_loads.load(Ordering::SeqCst), 2);
        assert!(
            cancellation_tokens
                .lock()
                .unwrap()
                .iter()
                .any(|(bucket, token)| bucket == "alpha-bucket"
                    && token.load(std::sync::atomic::Ordering::Acquire))
        );
    }

    #[gpui_kit::test]
    async fn sidebar_keyboard_controls_and_drag_respect_width_constraints(cx: &mut TestAppContext) {
        let (handle, main_window) = open_main_window(
            cx,
            fixture_state(Some("default"), &[]),
            fixture_commands(
                std::sync::Arc::new(AtomicUsize::new(0)),
                std::sync::Arc::new(AtomicUsize::new(0)),
            ),
            960.0,
            640.0,
        );

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bucket_list = main_window.read(cx).bucket_list.clone();
            bucket_list.update(cx, |list, cx| list.focus(window, cx));
            window.render_frame(cx);
            let initial =
                main_window.read(cx).resizable_state.read(cx).sizes()[0] / window.rem_size();
            assert!((initial - 22.0).abs() < 0.1);
            window.press("cmd-alt-right", cx);
            window.render_frame(cx);
            let widened =
                main_window.read(cx).resizable_state.read(cx).sizes()[0] / window.rem_size();
            assert!(
                (widened - 23.0).abs() < 0.1,
                "expected keyboard shortcut to widen 22rem sidebar to 23rem, got {widened}rem"
            );
            window.press("cmd-alt-0", cx);
            window.render_frame(cx);
            let reset =
                main_window.read(cx).resizable_state.read(cx).sizes()[0] / window.rem_size();
            assert!((reset - 22.0).abs() < 0.1);

            for _ in 0..20 {
                window.press("cmd-alt-right", cx);
            }
            window.render_frame(cx);
            let maximum =
                main_window.read(cx).resizable_state.read(cx).sizes()[0] / window.rem_size();
            assert!((maximum - 28.0).abs() < 0.1);

            let sidebar_bounds = window.find("sidebar-content").bounds();
            let from = gpui_kit::point(sidebar_bounds.right() - px(1.0), sidebar_bounds.center().y);
            window.drag(from, gpui_kit::point(from.x - px(24.0), from.y), cx);
            window.render_frame(cx);
            let dragged =
                main_window.read(cx).resizable_state.read(cx).sizes()[0] / window.rem_size();
            assert!(dragged <= 28.1);
            assert!(dragged >= 18.0);
            assert!(dragged < maximum);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    async fn minimum_and_default_window_sizes_render_in_light_and_dark_themes(
        cx: &mut TestAppContext,
    ) {
        let (handle, main_window) = open_main_window(
            cx,
            fixture_state(None, &[]),
            fixture_commands(
                std::sync::Arc::new(AtomicUsize::new(0)),
                std::sync::Arc::new(AtomicUsize::new(0)),
            ),
            900.0,
            600.0,
        );
        for theme in [ThemeMode::Light, ThemeMode::Dark] {
            cx.update(|cx| Theme::change(theme, None, cx));
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let rem_size = window.rem_size();
                let sizes = main_window
                    .read(cx)
                    .resizable_state
                    .read(cx)
                    .sizes()
                    .clone();
                assert!((sizes[0] / rem_size - 22.0).abs() < 0.1);
                assert!(sizes[1] / rem_size >= 30.0);
                assert!(
                    window.find("sidebar-content").bounds().size.width
                        >= super::rems(18.0).to_pixels(rem_size)
                );
                let list = main_window.read(cx).bucket_list.clone();
                list.update(cx, |list, cx| list.focus(window, cx));
                for _ in 0..20 {
                    window.press("cmd-alt-right", cx);
                }
                window.render_frame(cx);
                let sizes = main_window
                    .read(cx)
                    .resizable_state
                    .read(cx)
                    .sizes()
                    .clone();
                let sidebar_rem = sizes[0] / window.rem_size();
                assert!(sidebar_rem <= 26.35);
                assert!(sidebar_rem >= 18.0);
                assert!(sizes[1] / window.rem_size() >= 30.0);
                window.press("cmd-alt-0", cx);
            })
            .unwrap();
        }

        cx.simulate_window_resize(handle, size(px(960.0), px(640.0)));
        for theme in [ThemeMode::Light, ThemeMode::Dark] {
            cx.update(|cx| Theme::change(theme, None, cx));
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let rem_size = window.rem_size();
                let sizes = main_window
                    .read(cx)
                    .resizable_state
                    .read(cx)
                    .sizes()
                    .clone();
                assert!((18.0..=28.0).contains(&(sizes[0] / rem_size)));
                assert!(sizes[1] / rem_size >= 30.0);
                assert_eq!(window.bounds().size.width, px(960.0));
                assert_eq!(window.bounds().size.height, px(640.0));
                window.press("cmd-alt-0", cx);
                window.render_frame(cx);
                let reset = main_window.read(cx).resizable_state.read(cx).sizes()[0] / rem_size;
                assert!((reset - 22.0).abs() < 0.1);
            })
            .unwrap();
        }
    }
}
