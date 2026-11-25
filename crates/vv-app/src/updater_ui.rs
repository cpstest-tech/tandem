//! Update prompt: yes / not now / ignore this release, then the download and
//! the restart.

use super::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use updater::{Installed, Method, Relaunch, Release};

#[derive(Default)]
pub(crate) struct State {
    check: Option<Check>,
    dialog: Option<Dialog>,
    /// Run at exit; cleared if the quit is cancelled.
    pub relaunch: Option<Relaunch>,
}

struct Check {
    rx: mpsc::Receiver<Result<Option<Release>, String>>,
    /// From the menu: report "up to date" and errors, and ignore the skipped release.
    manual: bool,
}

enum Dialog {
    Offer(Release, Method),
    Downloading {
        release: Release,
        size: u64,
        progress: Arc<updater::Progress>,
        handle: std::thread::JoinHandle<Result<Installed, String>>,
    },
    Ready(Installed),
    Failed {
        release: Release,
        error: String,
    },
    UpToDate,
    CheckFailed(String),
}

enum Choice {
    Update,
    NotNow,
    Ignore,
    Cancel,
    Restart,
    Later,
    OpenPage(String),
    Close,
}

impl VenturiApp {
    pub(crate) fn start_update_check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.updater.check.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let _ = tx.send(updater::check());
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.updater.check = Some(Check { rx, manual });
        }
    }

    fn poll_update_check(&mut self) {
        let Some(check) = &self.updater.check else {
            return;
        };
        let result = match check.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("update check stopped".into()),
        };
        let manual = check.manual;
        self.updater.check = None;
        if self.updater.dialog.is_some() {
            return;
        }
        self.updater.dialog = match result {
            Ok(Some(release))
                if manual || self.settings.ignored_update.as_deref() != Some(&release.tag_name) =>
            {
                Some(Dialog::Offer(release, Method::detect()))
            }
            Ok(_) if manual => Some(Dialog::UpToDate),
            Err(error) if manual => Some(Dialog::CheckFailed(error)),
            Ok(_) => None,
            Err(error) => {
                eprintln!("update check failed: {error}");
                None
            }
        };
    }

    pub(crate) fn show_update_dialog(&mut self, ctx: &egui::Context) {
        self.poll_update_check();
        let Some(dialog) = &mut self.updater.dialog else {
            return;
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("update_dialog")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            choice = dialog_body(ui, dialog);
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(match dialog {
                Dialog::Offer(..) => Choice::NotNow,
                Dialog::Downloading { .. } => Choice::Cancel,
                Dialog::Ready(_) => Choice::Later,
                _ => Choice::Close,
            });
        }
        if let Dialog::Downloading { handle, .. } = dialog {
            if handle.is_finished() {
                let Some(Dialog::Downloading {
                    release, handle, ..
                }) = self.updater.dialog.take()
                else {
                    unreachable!()
                };
                self.updater.dialog = Some(match handle.join() {
                    Ok(Ok(installed)) => Dialog::Ready(installed),
                    Ok(Err(error)) if error == "Cancelled" => return,
                    Ok(Err(error)) => Dialog::Failed { release, error },
                    Err(_) => Dialog::Failed {
                        release,
                        error: "the update thread crashed".into(),
                    },
                });
                ctx.request_repaint();
                return;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(choice) = choice {
            self.apply_update_choice(ctx, choice);
        }
    }

    fn apply_update_choice(&mut self, ctx: &egui::Context, choice: Choice) {
        match choice {
            Choice::Update => {
                let Some(Dialog::Offer(release, method)) = self.updater.dialog.take() else {
                    return;
                };
                let Some(asset) = method.asset(&release).cloned() else {
                    ctx.open_url(egui::OpenUrl::new_tab(&release.html_url));
                    return;
                };
                let waker = ctx.clone();
                let (progress, handle) =
                    updater::spawn_download(method, asset.clone(), move || waker.request_repaint());
                self.updater.dialog = Some(Dialog::Downloading {
                    release,
                    size: asset.size,
                    progress,
                    handle,
                });
            }
            Choice::NotNow | Choice::Close => self.updater.dialog = None,
            Choice::Ignore => {
                if let Some(Dialog::Offer(release, _)) = self.updater.dialog.take() {
                    self.settings.ignored_update = Some(release.tag_name);
                    self.persist_settings();
                }
            }
            Choice::Cancel => {
                // The dialog closes when the thread notices.
                if let Some(Dialog::Downloading { progress, .. }) = &self.updater.dialog {
                    progress.cancel.store(true, Ordering::Relaxed);
                }
            }
            Choice::Restart | Choice::Later => {
                let Some(Dialog::Ready(installed)) = self.updater.dialog.take() else {
                    return;
                };
                let relaunch = match installed {
                    Installed::Replaced(exe) => Relaunch::App(exe),
                    Installed::Installer(setup) => Relaunch::Installer(setup),
                    Installed::DiskImage => return,
                };
                // A replaced AppImage is already the next launch's version.
                if matches!(choice, Choice::Restart) || matches!(relaunch, Relaunch::Installer(_)) {
                    self.updater.relaunch = Some(relaunch);
                }
                if matches!(choice, Choice::Restart) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Choice::OpenPage(url) => {
                ctx.open_url(egui::OpenUrl::new_tab(url));
                self.updater.dialog = None;
            }
        }
    }

    /// From `on_exit`.
    pub(crate) fn run_pending_relaunch(&mut self) {
        let Some(relaunch) = self.updater.relaunch.take() else {
            return;
        };
        let project = self.session.path().map(Path::to_path_buf);
        if let Err(e) = relaunch.spawn(project.as_deref()) {
            eprintln!("cannot start the updated Tandem: {e}");
        }
    }
}

fn dialog_body(ui: &mut egui::Ui, dialog: &mut Dialog) -> Option<Choice> {
    let mut choice = None;
    match dialog {
        Dialog::Offer(release, method) => {
            crate::ui_kit::heading(ui, t!("update.available_title"));
            ui.label(t!(
                "update.available",
                version = release.tag_name.trim_start_matches('v'),
                current = updater::current_version()
            ));
            ui.hyperlink_to(t!("update.release_notes"), &release.html_url);
            if method.asset(release).is_none() {
                ui.label(egui::RichText::new(t!("update.manual_hint")).weak());
            }
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                if ui.button(t!("update.yes")).clicked() {
                    choice = Some(Choice::Update);
                }
                if ui.button(t!("update.not_now")).clicked() {
                    choice = Some(Choice::NotNow);
                }
                if ui.button(t!("update.ignore")).clicked() {
                    choice = Some(Choice::Ignore);
                }
            });
        }
        Dialog::Downloading {
            release,
            size,
            progress,
            ..
        } => {
            crate::ui_kit::heading(
                ui,
                t!(
                    "update.downloading",
                    version = release.tag_name.trim_start_matches('v')
                ),
            );
            let done = progress.done.load(Ordering::Relaxed);
            ui.add(
                egui::ProgressBar::new(done as f32 / (*size).max(1) as f32)
                    .desired_width(380.0)
                    .text(format!(
                        "{:.0} / {:.0} MB",
                        done as f64 / 1e6,
                        *size as f64 / 1e6
                    )),
            );
            ui.add_space(crate::theme::SPACE_2);
            let cancelling = progress.cancel.load(Ordering::Relaxed);
            if ui
                .add_enabled(!cancelling, egui::Button::new(t!("common.cancel")))
                .clicked()
            {
                choice = Some(Choice::Cancel);
            }
        }
        Dialog::Ready(installed) => {
            crate::ui_kit::heading(ui, t!("update.ready_title"));
            match installed {
                Installed::Replaced(_) => {
                    ui.label(t!("update.ready_restart"));
                }
                Installed::Installer(_) => {
                    ui.label(t!("update.ready_installer"));
                }
                Installed::DiskImage => {
                    ui.label(t!("update.ready_dmg"));
                }
            }
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                if matches!(installed, Installed::DiskImage) {
                    if ui.button(t!("update.ok")).clicked() {
                        choice = Some(Choice::Close);
                    }
                    return;
                }
                if ui.button(t!("update.restart_now")).clicked() {
                    choice = Some(Choice::Restart);
                }
                if ui.button(t!("update.later")).clicked() {
                    choice = Some(Choice::Later);
                }
            });
        }
        Dialog::Failed { release, error } => {
            crate::ui_kit::heading(ui, t!("update.failed_title"));
            ui.label(t!("update.failed", error = error.as_str()));
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                if ui.button(t!("update.open_page")).clicked() {
                    choice = Some(Choice::OpenPage(release.html_url.clone()));
                }
                if ui.button(t!("common.close")).clicked() {
                    choice = Some(Choice::Close);
                }
            });
        }
        Dialog::UpToDate => {
            ui.label(t!(
                "update.up_to_date",
                version = updater::current_version()
            ));
            ui.add_space(crate::theme::SPACE_2);
            if ui.button(t!("update.ok")).clicked() {
                choice = Some(Choice::Close);
            }
        }
        Dialog::CheckFailed(error) => {
            crate::ui_kit::heading(ui, t!("update.failed_title"));
            ui.label(t!("update.check_failed", error = error.as_str()));
            ui.add_space(crate::theme::SPACE_2);
            if ui.button(t!("common.close")).clicked() {
                choice = Some(Choice::Close);
            }
        }
    }
    choice
}
