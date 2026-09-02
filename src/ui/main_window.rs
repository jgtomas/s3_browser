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
    sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem},
    spinner::Spinner,
    v_flex,
};

use crate::{
    app::{DownloaderApp, VersionLoadRequest},
    aws,
    models::{AppState, AppStatus, DownloadRequest, ObjectVersion},
};

const SIDEBAR_WIDTH_REMS: f32 = 22.0;
const CONTENT_MAX_WIDTH_REMS: f32 = 56.0;

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
    bucket_filter_input: Entity<InputState>,
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
        let bucket_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter buckets"));
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
                &bucket_filter_input,
                window,
                |_, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
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
            bucket_filter_input,
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
        self.bucket_filter_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.clear_version_select(window, cx);

        let requested_profile = profile.clone();
        let result_profile = profile;
        self.spawn_bucket_load(requested_profile, result_profile, window, cx);
    }

    fn refresh_buckets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = match self
            .application
            .update(cx, |application, cx| application.begin_bucket_refresh(cx))
        {
            Ok(profile) => profile,
            Err(_) => return,
        };

        self.spawn_bucket_load(profile.clone(), profile, window, cx);
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
        let previous_bucket = self.application.read(cx).state.selected_bucket.clone();
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { aws::cli::list_buckets(&requested_profile) })
                    .await;

                let _ = cx.update(|window, cx| {
                    let accepted = application.update(cx, |application, cx| {
                        application.finish_bucket_load(&result_profile, result, cx)
                    });

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

    fn render_sidebar(&self, state: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_foreground = cx.theme().muted_foreground;
        let profile_select = Select::new(&self.profile_select)
            .placeholder("Select an AWS profile")
            .disabled(state.profiles.is_empty())
            .w_full();
        let filter = self.bucket_filter_input.read(cx).value().to_string();
        let filtered_buckets = filter_buckets(&state.buckets, &filter);
        let selected_bucket_hidden = state.selected_bucket.as_deref().is_some_and(|selected| {
            !filter.trim().is_empty()
                && !filtered_buckets
                    .iter()
                    .any(|bucket| bucket.as_str() == selected)
        });
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

        let header = v_flex()
            .gap_3()
            .p_2()
            .w_full()
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
            .child(profile_select)
            .child(div().text_xs().text_color(muted_foreground).child(
                if state.selected_profile.is_some() {
                    "AWS CLI profile active"
                } else {
                    "Choose a profile to load buckets"
                },
            ))
            .child(if state.manual_bucket_entry {
                v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_medium()
                                    .child(format!("Buckets · {}", state.buckets.len())),
                            )
                            .child(refresh),
                    )
                    .child(div().text_xs().font_medium().child("Bucket name"))
                    .child(
                        Input::new(&self.bucket_name_input)
                            .cleanable(true)
                            .disabled(state.loading_buckets || state.downloading)
                            .w_full(),
                    )
                    .child(div().text_xs().text_color(muted_foreground).child(
                        "Bucket listing is unavailable for this profile. Enter a name manually.",
                    ))
                    .into_any_element()
            } else {
                v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_medium()
                                    .child(format!("Buckets · {}", state.buckets.len())),
                            )
                            .child(refresh),
                    )
                    .child(
                        Input::new(&self.bucket_filter_input)
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true)
                            .disabled(state.selected_profile.is_none())
                            .w_full(),
                    )
                    .child(if selected_bucket_hidden {
                        div()
                            .text_xs()
                            .text_color(muted_foreground)
                            .child("Selected bucket is hidden by the filter")
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .into_any_element()
            });

        let footer_label = if state.manual_bucket_entry {
            "Manual bucket entry".to_string()
        } else {
            format!("{} buckets loaded", state.buckets.len())
        };

        Sidebar::left()
            .collapsible(false)
            .w(rems(SIDEBAR_WIDTH_REMS))
            .header(header)
            .child(SidebarGroup::new("Buckets").child(self.render_bucket_menu(
                state,
                &filtered_buckets,
                &filter,
            )))
            .footer(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(footer_label),
            )
    }

    fn render_bucket_menu(
        &self,
        state: &AppState,
        filtered_buckets: &[String],
        filter: &str,
    ) -> SidebarMenu {
        let items = if state.loading_buckets && state.buckets.is_empty() {
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
        } else if filtered_buckets.is_empty() {
            vec![
                SidebarMenuItem::new(format!("No buckets match “{}”", filter.trim())).disable(true),
            ]
        } else {
            filtered_buckets
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

        div()
            .h_flex()
            .items_start()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(&state, cx))
            .child(
                v_flex().flex_1().h_full().overflow_hidden().child(
                    v_flex().size_full().overflow_y_scrollbar().child(
                        v_flex()
                            .w_full()
                            .max_w(rems(CONTENT_MAX_WIDTH_REMS))
                            .child(content),
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
    muted_foreground: gpui::Hsla,
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
    muted_foreground: gpui::Hsla,
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
    cx: &mut gpui::AsyncWindowContext,
) {
    let _ = cx.update(|_, cx| {
        application.update(cx, |application, cx| {
            application.set_error(error, cx);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{filter_buckets, readiness_for};
    use crate::models::AppState;

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
}
