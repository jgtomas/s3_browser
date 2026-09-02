use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, rems,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectItem, SelectState},
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    spinner::Spinner,
    v_flex,
};

use crate::{
    app::{DownloaderApp, VersionLoadRequest},
    aws,
    models::{AppState, AppStatus, DownloadRequest, ObjectVersion},
};

const SIDEBAR_WIDTH_REMS: f32 = 22.0;

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
    profile_select: Entity<SelectState<Vec<String>>>,
    version_select: Entity<SelectState<Vec<ObjectVersion>>>,
    bucket_name_input: Entity<InputState>,
    object_key_input: Entity<InputState>,
    destination_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let application = cx.new(|_| DownloaderApp::new());
        let profile_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
        let version_select =
            cx.new(|cx| SelectState::new(Vec::<ObjectVersion>::new(), None, window, cx));
        let bucket_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Enter bucket name"));
        let object_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("folder/object.txt"));
        let destination_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Choose a local destination"));

        let profiles_result = aws::profiles::load_profiles();
        let cli_result = aws::cli::check_aws_cli();
        let profile_items = profiles_result.clone().unwrap_or_default();

        application.update(cx, |application, cx| {
            application.initialize(profiles_result, cli_result, cx);
        });
        profile_select.update(cx, |select, cx| {
            select.set_items(profile_items, window, cx);
            cx.notify();
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &profile_select,
                window,
                |this, _, event: &SelectEvent<Vec<String>>, window, cx| {
                    if let SelectEvent::Confirm(Some(profile)) = event {
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
                &bucket_name_input,
                window,
                |this, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_manual_bucket(value, cx);
                        });
                    }
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
                    InputEvent::PressEnter { secondary: false } => {
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

        Self {
            application,
            profile_select,
            version_select,
            bucket_name_input,
            object_key_input,
            destination_input,
            _subscriptions: subscriptions,
        }
    }

    fn select_profile(&mut self, profile: String, window: &mut Window, cx: &mut Context<Self>) {
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
        self.clear_version_select(window, cx);

        let application = self.application.clone();
        let requested_profile = profile.clone();
        let result_profile = profile;
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { aws::cli::list_buckets(&requested_profile) })
                    .await;

                let _ = cx.update(|_, cx| {
                    application.update(cx, |application, cx| {
                        application.finish_bucket_load(&result_profile, result, cx);
                    });
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
                    .spawn(async move { aws::cli::list_object_versions(&profile, &bucket, &key) })
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

        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        aws::cli::download_object(
                            &profile,
                            &bucket,
                            &key,
                            version_id.as_deref(),
                            &destination,
                        )
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

    fn render_sidebar(&self, state: &AppState, muted_foreground: gpui::Hsla) -> impl IntoElement {
        let profile_disabled = state.profiles.is_empty();
        let profile_select = Select::new(&self.profile_select)
            .placeholder("Select a profile")
            .disabled(profile_disabled)
            .w_full();

        let header = if state.manual_bucket_entry {
            SidebarHeader::new()
                .child(
                    v_flex()
                        .gap_2()
                        .w_full()
                        .child(div().font_semibold().child("S3 Downloader"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted_foreground)
                                .child("AWS buckets"),
                        )
                        .child(
                            Input::new(&self.bucket_name_input)
                                .disabled(state.loading_buckets || state.downloading)
                                .w_full(),
                        ),
                )
                .into_any_element()
        } else {
            SidebarHeader::new()
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
                                        .child("AWS buckets"),
                                ),
                        ),
                )
                .into_any_element()
        };

        Sidebar::left()
            .collapsible(false)
            .w(rems(SIDEBAR_WIDTH_REMS))
            .header(header)
            .child(SidebarGroup::new("Buckets").child(self.render_bucket_menu(state)))
            .footer(
                SidebarFooter::new().child(
                    v_flex()
                        .gap_2()
                        .w_full()
                        .child(div().text_xs().font_medium().child("AWS Profile"))
                        .child(profile_select),
                ),
            )
    }

    fn render_bucket_menu(&self, state: &AppState) -> SidebarMenu {
        let items = if state.loading_buckets {
            vec![
                SidebarMenuItem::new("Loading buckets…")
                    .disable(true)
                    .suffix(Spinner::new().xsmall()),
            ]
        } else if state.selected_profile.is_none() {
            vec![SidebarMenuItem::new("Select a profile to list buckets").disable(true)]
        } else if state.manual_bucket_entry {
            vec![SidebarMenuItem::new("Enter a bucket name above").disable(true)]
        } else if state.buckets.is_empty() {
            let message = match &state.status {
                Some(AppStatus::Error(message)) => message.clone(),
                _ => "No S3 buckets found for this profile".to_string(),
            };
            vec![SidebarMenuItem::new(message).disable(true)]
        } else {
            state
                .buckets
                .iter()
                .cloned()
                .map(|bucket| {
                    let active = state.selected_bucket.as_deref() == Some(bucket.as_str());
                    let application = self.application.clone();
                    let requested_bucket = bucket.clone();
                    SidebarMenuItem::new(bucket)
                        .icon(IconName::Folder)
                        .active(active)
                        .on_click(move |_, _, cx| {
                            application.update(cx, |application, cx| {
                                application.select_bucket(requested_bucket.clone(), cx);
                            });
                        })
                })
                .collect()
        };

        SidebarMenu::new().children(items)
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.application.read(cx).state.clone();
        let has_bucket = state.selected_bucket.is_some();
        let form_disabled = !has_bucket || state.downloading;
        let version_disabled = form_disabled
            || state.loading_versions
            || state.versions.is_empty()
            || state.object_key.trim().is_empty();
        let download_disabled = form_disabled
            || state.loading_versions
            || state.object_key.trim().is_empty()
            || state.destination.trim().is_empty();

        let version_control = if state.loading_versions {
            h_flex()
                .gap_2()
                .child(Spinner::new())
                .child("Loading versions…")
                .into_any_element()
        } else {
            Select::new(&self.version_select)
                .placeholder("Use the current version")
                .cleanable(true)
                .disabled(version_disabled)
                .w_full()
                .into_any_element()
        };
        let version_hint = if !has_bucket {
            "Select a bucket in the Sidebar to enable object downloads."
        } else if state.loading_versions {
            "Loading versions…"
        } else if state.object_key.trim().is_empty() {
            "Press Enter after entering a key to load its versions."
        } else if state.versions.is_empty() {
            "No versions loaded. Download will use the current version."
        } else {
            "Leave the version empty to download the current version."
        };

        let selected_bucket = state
            .selected_bucket
            .as_deref()
            .map(|bucket| format!("Bucket: {bucket}"))
            .unwrap_or_else(|| "Choose a bucket from the Sidebar".to_string());

        let status = status_element(state.status.clone());
        let content = v_flex()
            .gap_4()
            .p_8()
            .w_full()
            .child(div().text_xl().font_semibold().child("Download an object"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Use the AWS CLI to download an S3 object and optionally choose its version."),
            )
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .text_color(cx.theme().foreground)
                    .child(selected_bucket),
            )
            .child(field(
                "Object key",
                Input::new(&self.object_key_input)
                    .disabled(form_disabled)
                    .w_full()
                    .into_any_element(),
            ))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(version_hint))
            .child(field("Version (optional)", version_control))
            .child(field(
                "Destination",
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        Input::new(&self.destination_input)
                            .disabled(form_disabled)
                            .flex_1(),
                    )
                    .child(
                        Button::new("browse")
                            .label("Browse")
                            .disabled(form_disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_destination(window, cx);
                            })),
                    ),
            ))
            .child(
                Button::new("download")
                    .primary()
                    .label("Download")
                    .loading(state.downloading)
                    .disabled(download_disabled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_download(window, cx);
                    })),
            )
            .child(status);

        div()
            .h_flex()
            .items_start()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(&state, cx.theme().muted_foreground))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(v_flex().size_full().overflow_y_scrollbar().child(content)),
            )
    }
}

fn field(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .w_full()
        .child(div().text_sm().font_medium().child(label))
        .child(control)
}

fn status_element(status: Option<AppStatus>) -> AnyElement {
    match status {
        Some(AppStatus::Info(message)) => Alert::info("status-info", message).into_any_element(),
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
    cx: &mut gpui::AsyncWindowContext,
) {
    let _ = cx.update(|_, cx| {
        application.update(cx, |application, cx| {
            application.set_error(error, cx);
        });
    });
}
