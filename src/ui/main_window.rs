use std::path::{Path, PathBuf};

use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt,
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    select::{Select, SelectEvent, SelectState},
    spinner::Spinner,
    v_flex,
};

use crate::{
    app::DownloaderApp,
    aws,
    models::{AppStatus, DownloadRequest},
};

pub struct MainWindow {
    application: Entity<DownloaderApp>,
    profile_select: Entity<SelectState<Vec<String>>>,
    bucket_select: Entity<SelectState<Vec<String>>>,
    object_key_input: Entity<InputState>,
    version_id_input: Entity<InputState>,
    destination_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let application = cx.new(|_| DownloaderApp::new());
        let profile_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
        let bucket_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
        let object_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("folder/object.txt"));
        let version_id_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Leave blank for the current version")
        });
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
                &bucket_select,
                window,
                |this, _, event: &SelectEvent<Vec<String>>, _, cx| {
                    if let SelectEvent::Confirm(Some(bucket)) = event {
                        this.application.update(cx, |application, cx| {
                            application.select_bucket(bucket.clone(), cx);
                        });
                    }
                },
            ),
            cx.subscribe_in(
                &object_key_input,
                window,
                |this, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_object_key(value, cx);
                        });
                    }
                },
            ),
            cx.subscribe_in(
                &version_id_input,
                window,
                |this, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        this.application.update(cx, |application, cx| {
                            application.set_version_id(value, cx);
                        });
                    }
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
            bucket_select,
            object_key_input,
            version_id_input,
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

        self.bucket_select.update(cx, |select, cx| {
            select.set_items(Vec::new(), window, cx);
            select.set_selected_index(None, window, cx);
            cx.notify();
        });

        let application = self.application.clone();
        let bucket_select = self.bucket_select.clone();
        let requested_profile = profile.clone();
        let result_profile = profile;
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { aws::cli::list_buckets(&requested_profile) })
                    .await;
                let buckets = result.clone().unwrap_or_default();

                let _ = cx.update(|window, cx| {
                    let accepted = application.update(cx, |application, cx| {
                        application.finish_bucket_load(&result_profile, result, cx)
                    });

                    if accepted {
                        bucket_select.update(cx, |select, cx| {
                            select.set_items(buckets, window, cx);
                            select.set_selected_index(None, window, cx);
                            cx.notify();
                        });
                    }
                });
            })
            .detach();
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
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.application.read(cx).state.clone();
        let downloading = state.downloading;
        let profile_disabled = downloading || state.profiles.is_empty();
        let bucket_disabled = downloading
            || state.loading_buckets
            || state.selected_profile.is_none()
            || state.buckets.is_empty();

        let status = status_element(state.status.clone());
        let bucket_control = if state.loading_buckets {
            h_flex()
                .gap_2()
                .child(Spinner::new())
                .child("Loading buckets...")
                .into_any_element()
        } else {
            Select::new(&self.bucket_select)
                .placeholder("Select a bucket")
                .disabled(bucket_disabled)
                .w_full()
                .into_any_element()
        };

        div()
            .v_flex()
            .gap_4()
            .size_full()
            .p_8()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().text_size(px(24.0)).child("S3 Downloader"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Download an object from an S3 bucket using your local AWS CLI."),
            )
            .child(field(
                "AWS Profile",
                Select::new(&self.profile_select)
                    .placeholder("Select a profile")
                    .disabled(profile_disabled)
                    .w_full()
                    .into_any_element(),
            ))
            .child(field("S3 Bucket", bucket_control))
            .child(field(
                "Object key",
                Input::new(&self.object_key_input)
                    .disabled(downloading)
                    .w_full()
                    .into_any_element(),
            ))
            .child(field(
                "Version ID (optional)",
                Input::new(&self.version_id_input)
                    .disabled(downloading)
                    .w_full()
                    .into_any_element(),
            ))
            .child(field(
                "Destination",
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        Input::new(&self.destination_input)
                            .disabled(downloading)
                            .flex_1(),
                    )
                    .child(
                        Button::new("browse")
                            .label("Browse")
                            .disabled(downloading)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_destination(window, cx);
                            })),
                    ),
            ))
            .child(
                Button::new("download")
                    .primary()
                    .label("Download")
                    .loading(downloading)
                    .disabled(downloading)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_download(window, cx);
                    })),
            )
            .child(status)
    }
}

fn field(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .w_full()
        .child(div().text_size(px(13.0)).child(label))
        .child(control)
}

fn status_element(status: Option<AppStatus>) -> gpui::AnyElement {
    match status {
        Some(AppStatus::Info(message)) => Alert::info("status-info", message).into_any_element(),
        Some(AppStatus::Success(message)) => {
            Alert::success("status-success", message).into_any_element()
        }
        Some(AppStatus::Error(message)) => Alert::error("status-error", message).into_any_element(),
        None => div().into_any_element(),
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
