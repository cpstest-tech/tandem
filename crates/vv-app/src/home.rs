//! The projects Home: a card per project folder, with search, sort and a
//! format filter; and the steps that open, create and leave a project.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use super::*;
use crate::projects::{self, Aspect, ProjectFsError, ProjectSummary};

const CARD_WIDTH: f32 = 220.0;
const COVER_HEIGHT: f32 = CARD_WIDTH * 9.0 / 16.0;
const CARD_HEIGHT: f32 = COVER_HEIGHT + 64.0;
const CARD_GAP: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HomeSort {
    #[default]
    Recent,
    Name,
    Size,
}

impl HomeSort {
    const ALL: [HomeSort; 3] = [HomeSort::Recent, HomeSort::Name, HomeSort::Size];

    fn label(self) -> Cow<'static, str> {
        match self {
            HomeSort::Recent => t!("home.sort_recent"),
            HomeSort::Name => t!("home.sort_name"),
            HomeSort::Size => t!("home.sort_size"),
        }
    }
}

fn aspect_label(aspect: Option<Aspect>) -> Cow<'static, str> {
    match aspect {
        None => t!("home.format_all"),
        Some(Aspect::Wide) => "16:9".into(),
        Some(Aspect::Vertical) => "9:16".into(),
        Some(Aspect::Square) => "1:1".into(),
        Some(Aspect::Classic) => "4:3".into(),
        Some(Aspect::Other) => t!("home.format_other"),
        Some(Aspect::Undefined) => t!("home.format_undefined"),
    }
}

pub(crate) struct HomeCard {
    pub(crate) summary: ProjectSummary,
    cover: Option<egui::TextureHandle>,
}

struct RenameDialog {
    file: PathBuf,
    name: String,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum HomeAction {
    NewProject,
    ImportProject,
    ChooseFolder,
    Open(PathBuf),
    Rename(PathBuf),
    Duplicate(PathBuf),
    Delete(PathBuf),
    Reveal(PathBuf),
}

#[derive(Default)]
pub(crate) struct HomeState {
    /// The folder the cards come from; `None` until read, or to read again.
    scanned: Option<PathBuf>,
    cards: Vec<HomeCard>,
    scan: Option<mpsc::Receiver<ProjectSummary>>,
    search: String,
    sort: HomeSort,
    aspect: Option<Aspect>,
    rename: Option<RenameDialog>,
    confirm_delete: Option<PathBuf>,
    error: Option<String>,
}

impl HomeState {
    pub(crate) fn invalidate(&mut self) {
        self.scanned = None;
    }

    fn rescan(&mut self, root: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let dir = root.clone();
        std::thread::spawn(move || {
            let files = std::sync::Mutex::new(projects::list_projects(&dir));
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    let tx = tx.clone();
                    let files = &files;
                    scope.spawn(move || {
                        while let Some(file) = files.lock().ok().and_then(|mut f| f.pop()) {
                            if tx.send(projects::summarize(&file)).is_err() {
                                return;
                            }
                        }
                    });
                }
            });
        });
        self.cards.clear();
        self.scan = Some(rx);
        self.scanned = Some(root);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.scan else {
            return;
        };
        let mut received = Vec::new();
        let finished = loop {
            match rx.try_recv() {
                Ok(summary) => received.push(summary),
                Err(mpsc::TryRecvError::Empty) => break false,
                Err(mpsc::TryRecvError::Disconnected) => break true,
            }
        };
        if finished {
            self.scan = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        for summary in received {
            let cover = summary.cover.as_ref().map(|cover| {
                ctx.load_texture(
                    format!("project-cover-{}", summary.file.display()),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [cover.width as usize, cover.height as usize],
                        &cover.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
            self.cards.push(HomeCard { summary, cover });
        }
    }
}

/// Indices of the cards to show, in order.
fn visible(cards: &[HomeCard], search: &str, sort: HomeSort, aspect: Option<Aspect>) -> Vec<usize> {
    let needle = search.trim().to_lowercase();
    let mut shown: Vec<usize> = cards
        .iter()
        .enumerate()
        .filter(|(_, card)| needle.is_empty() || card.summary.name.to_lowercase().contains(&needle))
        .filter(|(_, card)| aspect.is_none_or(|a| card.summary.aspect == a))
        .map(|(index, _)| index)
        .collect();
    let summary = |index: usize| &cards[index].summary;
    match sort {
        HomeSort::Recent => shown.sort_by_key(|&index| std::cmp::Reverse(summary(index).modified)),
        HomeSort::Name => shown.sort_by_key(|&index| summary(index).name.to_lowercase()),
        HomeSort::Size => shown.sort_by_key(|&index| std::cmp::Reverse(summary(index).size_bytes)),
    }
    shown
}

pub(crate) fn fs_error_message(error: &ProjectFsError) -> String {
    match error {
        ProjectFsError::InvalidName => t!("home.invalid_name").into_owned(),
        ProjectFsError::AlreadyExists => t!("home.name_taken").into_owned(),
        ProjectFsError::NotAProjectFolder => t!("home.not_a_project_folder").into_owned(),
        ProjectFsError::Io(e) => t!("home.fs_error", error = e).into_owned(),
        ProjectFsError::Persistence(e) => t!("home.fs_error", error = e).into_owned(),
    }
}

impl VenturiApp {
    pub(crate) fn home_ui(&mut self, ui: &mut egui::Ui) {
        // An agent may have opened a project meanwhile.
        if self.session.path().is_some() {
            self.screen = Screen::Editor;
            ui.ctx().request_repaint();
            return;
        }
        let ctx = ui.ctx().clone();
        self.sync_window_title(&ctx);
        self.import_dropped_projects(&ctx);
        let root = self.settings.projects_dir();
        if self.home.scanned != root
            && let Some(root) = root.clone()
        {
            self.home.rescan(root);
        }
        self.home.poll(&ctx);

        let mut action = None;
        egui::Panel::top("home_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::symmetric(
                        theme::SPACE_4 as i8,
                        theme::SPACE_2 as i8,
                    ))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER_SUBTLE)),
            )
            .show(ui, |ui| self.home_bar(ui, root.as_deref(), &mut action));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::same(theme::SPACE_5 as i8)),
            )
            .show(ui, |ui| {
                paint_home_backdrop(ui);
                if home_banner(ui) {
                    action = Some(HomeAction::NewProject);
                }
                ui.add_space(theme::SPACE_5);
                self.home_filters(ui);
                ui.add_space(theme::SPACE_3);
                self.home_grid(ui, &mut action);
            });
        self.show_rename_dialog(&ctx);
        self.show_delete_dialog(&ctx);
        if let Some(action) = action {
            self.run_home_action(action);
        }
    }

    fn home_bar(
        &mut self,
        ui: &mut egui::Ui,
        root: Option<&Path>,
        action: &mut Option<HomeAction>,
    ) {
        ui.horizontal(|ui| {
            ui.set_min_height(theme::FIELD_HEIGHT);
            crate::ui_kit::heading(ui, "Tandem");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(t!("home.import_project"))
                    .on_hover_text(t!("home.import_project_hint"))
                    .clicked()
                {
                    *action = Some(HomeAction::ImportProject);
                }
                if ui.button(t!("home.change_folder")).clicked() {
                    *action = Some(HomeAction::ChooseFolder);
                }
                let folder = root.map_or_else(
                    || t!("home.no_folder").into_owned(),
                    |root| root.display().to_string(),
                );
                ui.add(
                    egui::Label::new(crate::ui_kit::hint(t!("home.folder", folder = folder)))
                        .truncate(),
                );
            });
        });
    }

    /// Section title on the left, search and filters on the right.
    fn home_filters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(crate::ui_kit::section_title(t!("home.projects_section")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let aspect = &mut self.home.aspect;
                egui::ComboBox::from_id_salt("home_format")
                    .selected_text(t!("home.format", format = aspect_label(*aspect)))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(aspect, None, aspect_label(None));
                        for option in Aspect::ALL {
                            ui.selectable_value(aspect, Some(option), aspect_label(Some(option)));
                        }
                    });
                let sort = &mut self.home.sort;
                egui::ComboBox::from_id_salt("home_sort")
                    .selected_text(t!("home.sort", order = sort.label()))
                    .show_ui(ui, |ui| {
                        for option in HomeSort::ALL {
                            ui.selectable_value(sort, option, option.label());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.home.search)
                        .hint_text(t!("home.search_hint"))
                        .desired_width(220.0),
                );
            });
        });
    }

    fn home_grid(&mut self, ui: &mut egui::Ui, action: &mut Option<HomeAction>) {
        if let Some(error) = &self.home.error {
            ui.colored_label(theme::ERROR, error);
        }
        let home = &self.home;
        let shown = visible(&home.cards, &home.search, home.sort, home.aspect);
        if shown.is_empty() {
            if home.scan.is_some() {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                if home.cards.is_empty() {
                    crate::ui_kit::heading(ui, t!("home.empty"));
                } else {
                    ui.label(t!("home.no_match"));
                }
            });
            return;
        }
        let columns = ((ui.available_width() + CARD_GAP) / (CARD_WIDTH + CARD_GAP))
            .floor()
            .max(1.0) as usize;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = CARD_GAP;
                        for &index in row {
                            if let Some(chosen) = card_ui(ui, &home.cards[index]) {
                                *action = Some(chosen);
                            }
                        }
                    });
                    ui.add_space(CARD_GAP);
                }
            });
    }

    fn run_home_action(&mut self, action: HomeAction) {
        self.home.error = None;
        match action {
            HomeAction::NewProject => self.create_new_project(),
            HomeAction::ImportProject => {
                self.spawn_file_dialog(DialogKind::ImportProject, |dlg| {
                    dlg.add_filter(t!("file_filter.project"), &[projects::PROJECT_EXTENSION])
                        .pick_file()
                });
            }
            HomeAction::ChooseFolder => {
                self.spawn_file_dialog(DialogKind::ProjectsFolder, |dlg| dlg.pick_folder());
            }
            HomeAction::Open(file) => self.request_project_switch(ProjectSwitch::OpenPath(file)),
            HomeAction::Rename(file) => {
                self.home.rename = Some(RenameDialog {
                    name: projects::project_name(&file),
                    file,
                    error: None,
                });
            }
            HomeAction::Duplicate(file) => {
                match projects::duplicate_project(&file, &t!("home.copy_suffix")) {
                    Ok(_) => self.home.invalidate(),
                    Err(e) => self.home.error = Some(fs_error_message(&e)),
                }
            }
            HomeAction::Delete(file) => self.home.confirm_delete = Some(file),
            HomeAction::Reveal(file) => {
                if let Err(e) = reveal_in_file_manager(file.parent().unwrap_or(&file)) {
                    self.home.error = Some(t!("home.fs_error", error = e).into_owned());
                }
            }
        }
    }

    fn show_rename_dialog(&mut self, ctx: &egui::Context) {
        let ai_busy = self
            .home
            .rename
            .as_ref()
            .is_some_and(|dialog| self.ai_project_busy(&dialog.file));
        let Some(dialog) = self.home.rename.as_mut() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("home_rename")).show(ctx, |ui| {
            crate::ui_kit::heading(ui, t!("home.rename_title"));
            ui.add_space(crate::theme::SPACE_1);
            let edit = ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(280.0));
            edit.request_focus();
            confirm = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(error) = &dialog.error {
                ui.colored_label(theme::ERROR, error);
            }
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                confirm |= ui.button(t!("home.rename")).clicked();
                cancel = ui.button(t!("common.cancel")).clicked();
            });
        });
        let mut renamed = false;
        if confirm && ai_busy {
            dialog.error = Some(
                "Wait for AI generation to finish or cancel its jobs before renaming the project"
                    .into(),
            );
        } else if confirm {
            match projects::rename_project(&dialog.file, &dialog.name) {
                Ok(_) => renamed = true,
                Err(e) => dialog.error = Some(fs_error_message(&e)),
            }
        }
        if renamed {
            let path = dialog.file.clone();
            self.release_ai_project(&path);
        }
        if renamed || cancel || modal.should_close() {
            self.home.rename = None;
        }
        if renamed {
            self.home.invalidate();
        }
    }

    fn show_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(file) = self.home.confirm_delete.clone() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("home_delete")).show(ctx, |ui| {
            crate::ui_kit::heading(
                ui,
                t!("home.delete_title", name = projects::project_name(&file)),
            );
            ui.label(t!("home.delete_detail"));
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                let delete = egui::Button::new(
                    egui::RichText::new(t!("home.delete")).color(egui::Color32::WHITE),
                )
                .fill(theme::ERROR);
                confirm = ui.add(delete).clicked();
                cancel = ui.button(t!("common.cancel")).clicked();
            });
        });
        if confirm && self.ai_project_busy(&file) {
            self.home.error = Some(
                "Wait for AI generation to finish or cancel its jobs before deleting the project"
                    .into(),
            );
        } else if confirm {
            self.release_ai_project(&file);
            match projects::delete_project(&file) {
                Ok(()) => self.home.invalidate(),
                Err(e) => self.home.error = Some(fs_error_message(&e)),
            }
        }
        if confirm || cancel || modal.should_close() {
            self.home.confirm_delete = None;
        }
    }

    fn import_dropped_projects(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case(projects::PROJECT_EXTENSION))
                })
                .collect()
        });
        if let Some(src) = dropped.first() {
            self.import_and_open_project(src);
        }
    }

    fn report_project_error(&mut self, message: String) {
        match self.screen {
            Screen::Home => self.home.error = Some(message),
            Screen::Editor => self.project_error = Some(message),
        }
    }

    /// `Progetto_NN` in the projects folder, created and opened at once.
    pub(crate) fn create_new_project(&mut self) {
        let Some(root) = self.settings.projects_dir() else {
            self.report_project_error(t!("home.no_folder").into_owned());
            return;
        };
        let name = projects::next_project_name(&root, &t!("home.new_project_prefix"));
        match projects::create_project(&root, &name) {
            Ok(file) => self.load_project_from(file),
            Err(e) => self.report_project_error(fs_error_message(&e)),
        }
    }

    /// A project file from anywhere becomes a project of the Home, then opens.
    pub(crate) fn import_and_open_project(&mut self, src: &Path) {
        let Some(root) = self.settings.projects_dir() else {
            self.report_project_error(t!("home.no_folder").into_owned());
            return;
        };
        match projects::import_project(src, &root) {
            Ok(file) => {
                self.home.invalidate();
                self.request_project_switch(ProjectSwitch::OpenPath(file));
            }
            Err(e) => self.report_project_error(fs_error_message(&e)),
        }
    }

    /// Called once the project is saved: the editor is left empty.
    pub(crate) fn close_project_to_home(&mut self) {
        self.new_project();
        self.mark_clean_exit();
        self.screen = Screen::Home;
        self.home.invalidate();
    }

    pub(crate) fn enter_editor(&mut self, project: PathBuf) {
        self.screen = Screen::Editor;
        self.settings.open_project = Some(project);
        self.persist_settings();
    }

    /// At startup with nothing to open: the Home, unless the last run ended
    /// with a project open, which is then reopened from its last save.
    pub(crate) fn start_on_home_or_recover(&mut self) {
        self.screen = Screen::Home;
        let Some(path) = self.settings.open_project.clone() else {
            return;
        };
        let _ = std::fs::remove_file(vv_core::persistence::temp_path(&path));
        self.load_project_from(path.clone());
        if self.session.path() == Some(path.as_path()) {
            self.recovered_project = Some(projects::project_name(&path));
        } else {
            self.project_error = None;
            self.mark_clean_exit();
        }
    }

    pub(crate) fn show_recovery_notice(&mut self, ctx: &egui::Context) {
        let Some(name) = self.recovered_project.clone() else {
            return;
        };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("recovered_project")).show(ctx, |ui| {
            crate::ui_kit::heading(ui, t!("project.recovered_title"));
            ui.label(t!("project.recovered", name = name));
            ui.add_space(crate::theme::SPACE_2);
            close = ui.button(t!("common.close")).clicked();
        });
        if close || modal.should_close() {
            self.recovered_project = None;
        }
    }

    /// From the top bar: the folder and the file follow the new name, and
    /// so do the media copied inside the folder.
    pub(crate) fn rename_open_project(&mut self, new_name: &str) {
        let Some(old_file) = self.session.path().map(Path::to_path_buf) else {
            return;
        };
        if self.ai_project_busy(&old_file) {
            self.project_error = Some(
                "Wait for AI generation to finish or cancel its jobs before renaming the project"
                    .into(),
            );
            return;
        }
        self.release_ai_project(&old_file);
        if projects::project_name(&old_file) == new_name.trim() || !self.flush_autosave() {
            return;
        }
        let managed = self
            .settings
            .projects_dir()
            .is_some_and(|root| projects::is_in_projects_folder(&old_file, &root));
        let renamed = if managed {
            projects::rename_project(&old_file, new_name)
        } else {
            projects::rename_loose_file(&old_file, new_name)
        };
        let new_file = match renamed {
            Ok(file) => file,
            Err(e) => {
                self.project_error = Some(fs_error_message(&e));
                return;
            }
        };
        crate::ai_sequence::portable::rebase(&mut self.session.project, &old_file, &new_file);
        if let (Some(old_dir), Some(new_dir)) = (old_file.parent(), new_file.parent()) {
            projects::rebase_media_paths(&mut self.session.project, old_dir, new_dir);
        }
        self.session.set_path(Some(new_file.clone()));
        self.settings.recent_projects.retain(|p| p != &old_file);
        self.remember_recent_project(new_file.clone());
        self.enter_editor(new_file);
        self.media_pool_state.invalidate_offline();
        if let (Some(render_ahead), Some(timeline_id)) = (&self.render_ahead, self.timeline_id) {
            render_ahead.update_project(&self.session.project, timeline_id);
        }
    }
}

const BANNER_HEIGHT: f32 = 145.0;
const BANNER_FROM: egui::Color32 = egui::Color32::from_rgb(0x26, 0xc6, 0xda);
const BANNER_TO: egui::Color32 = egui::Color32::from_rgb(0x8f, 0xe3, 0xf0);
const BANNER_TEXT: egui::Color32 = egui::Color32::from_rgb(0x10, 0x20, 0x24);
const HOME_GLOW: egui::Color32 = egui::Color32::from_rgb(0x0b, 0x16, 0x18);

fn paint_home_backdrop(ui: &egui::Ui) {
    let full = ui.max_rect();
    let rect = egui::Rect::from_min_size(
        full.min - egui::vec2(theme::SPACE_4, theme::SPACE_4),
        egui::vec2(full.width() + 2.0 * theme::SPACE_4, 220.0),
    );
    let mut mesh = egui::Mesh::default();
    let (top, bottom) = (HOME_GLOW, theme::BG_WINDOW);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn home_banner(ui: &mut egui::Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), BANNER_HEIGHT),
        egui::Sense::click(),
    );
    const STRIPS: usize = 48;
    let radius = theme::RADIUS_MODAL as u8;
    let painter = ui.painter();
    for i in 0..STRIPS {
        let (x0, x1) = (
            rect.left() + rect.width() * i as f32 / STRIPS as f32,
            rect.left() + rect.width() * (i + 1) as f32 / STRIPS as f32 + 0.5,
        );
        let color = BANNER_FROM.lerp_to_gamma(BANNER_TO, i as f32 / (STRIPS - 1) as f32);
        let color = if response.hovered() {
            color
                .gamma_multiply(1.0)
                .lerp_to_gamma(egui::Color32::WHITE, 0.1)
        } else {
            color
        };
        let corner = egui::CornerRadius {
            nw: if i == 0 { radius } else { 0 },
            sw: if i == 0 { radius } else { 0 },
            ne: if i == STRIPS - 1 { radius } else { 0 },
            se: if i == STRIPS - 1 { radius } else { 0 },
        };
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(x0..=x1, rect.y_range()),
            corner,
            color,
        );
    }
    let center = rect.center();
    let plus = egui::Rect::from_center_size(
        egui::pos2(center.x - 78.0, center.y),
        egui::Vec2::splat(26.0),
    );
    painter.rect_filled(plus, theme::RADIUS_CONTROL, BANNER_TEXT);
    let stroke = egui::Stroke::new(2.0, BANNER_FROM);
    painter.line_segment(
        [
            plus.center() - egui::vec2(6.0, 0.0),
            plus.center() + egui::vec2(6.0, 0.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            plus.center() - egui::vec2(0.0, 6.0),
            plus.center() + egui::vec2(0.0, 6.0),
        ],
        stroke,
    );
    painter.text(
        egui::pos2(plus.right() + theme::SPACE_3, center.y),
        egui::Align2::LEFT_CENTER,
        t!("home.new_project"),
        egui::FontId::proportional(crate::theme::FONT_DISPLAY),
        BANNER_TEXT,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn card_ui(ui: &mut egui::Ui, card: &HomeCard) -> Option<HomeAction> {
    let summary = &card.summary;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(CARD_WIDTH, CARD_HEIGHT), egui::Sense::click());
    let visuals = ui.visuals().clone();
    let cover_rect = egui::Rect::from_min_size(rect.min, egui::vec2(CARD_WIDTH, COVER_HEIGHT));
    ui.painter()
        .rect_filled(cover_rect, theme::RADIUS_PANEL, theme::BG_INSET);
    match &card.cover {
        Some(texture) => {
            let size = texture.size_vec2();
            let scale = (cover_rect.width() / size.x).min(cover_rect.height() / size.y);
            ui.painter().image(
                texture.id(),
                egui::Rect::from_center_size(cover_rect.center(), size * scale),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter().text(
                cover_rect.center(),
                egui::Align2::CENTER_CENTER,
                t!("home.no_preview"),
                egui::FontId::proportional(theme::FONT_SMALL),
                visuals.weak_text_color(),
            );
        }
    }
    if response.hovered() {
        ui.painter().rect_stroke(
            cover_rect,
            theme::RADIUS_PANEL,
            egui::Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }

    let mut chosen = None;
    let menu_rect = egui::Rect::from_min_size(
        egui::pos2(cover_rect.right() - 34.0, cover_rect.top() + 6.0),
        egui::vec2(28.0, 22.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
        ui.menu_button("…", |ui| card_menu(ui, &summary.file, &mut chosen))
            .response
            .on_hover_text(t!("home.card_menu"));
    });
    response.context_menu(|ui| card_menu(ui, &summary.file, &mut chosen));

    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, cover_rect.bottom() + theme::SPACE_2),
        rect.max - egui::vec2(2.0, 0.0),
    );
    // The card sits in a horizontal row: without an explicit layout the lines
    // would be laid out side by side.
    let text_ui = egui::UiBuilder::new()
        .max_rect(text_rect)
        .layout(egui::Layout::top_down(egui::Align::Min));
    ui.scope_builder(text_ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(
            egui::Label::new(
                egui::RichText::new(&summary.name)
                    .strong()
                    .size(theme::FONT_BODY)
                    .color(theme::TEXT_PRIMARY),
            )
            .truncate()
            .selectable(false),
        );
        let duration = summary
            .duration_secs
            .map_or_else(|| "—".to_owned(), format_duration);
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{} | {duration}", format_size(summary.size_bytes)))
                    .small()
                    .weak(),
            )
            .truncate()
            .selectable(false),
        );
        match &summary.error {
            Some(error) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("home.unreadable"))
                            .small()
                            .color(theme::ERROR),
                    )
                    .selectable(false),
                )
                .on_hover_text(error);
            }
            None => {
                if let Some(modified) = summary.modified {
                    ui.add(
                        egui::Label::new(egui::RichText::new(format_date(modified)).small().weak())
                            .truncate()
                            .selectable(false),
                    );
                }
            }
        }
    });

    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if chosen.is_none() && response.clicked() {
        chosen = Some(HomeAction::Open(summary.file.clone()));
    }
    chosen
}

fn card_menu(ui: &mut egui::Ui, file: &Path, chosen: &mut Option<HomeAction>) {
    let entries = [
        (t!("home.open"), HomeAction::Open(file.to_path_buf())),
        (t!("home.rename"), HomeAction::Rename(file.to_path_buf())),
        (
            t!("home.duplicate"),
            HomeAction::Duplicate(file.to_path_buf()),
        ),
        (t!("home.delete"), HomeAction::Delete(file.to_path_buf())),
        (t!("home.reveal"), HomeAction::Reveal(file.to_path_buf())),
    ];
    for (label, action) in entries {
        if ui.button(label).clicked() {
            *chosen = Some(action);
            ui.close();
        }
    }
}

pub(crate) fn reveal_in_file_manager(dir: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program).arg(dir).spawn()?;
    // Reaped in the background, or it would linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn format_duration(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_date(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (year, month, day, hour, minute) = local_time(secs);
    t!(
        "home.date",
        day = format!("{day:02}"),
        month = format!("{month:02}"),
        year = year,
        hour = format!("{hour:02}"),
        minute = format!("{minute:02}")
    )
    .into_owned()
}

#[cfg(unix)]
fn local_time(secs: i64) -> (i64, u32, u32, u32, u32) {
    let time = secs as libc::time_t;
    // SAFETY: `tm` is plain data, `localtime_r` only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return utc_time(secs);
    }
    (
        i64::from(tm.tm_year) + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    )
}

#[cfg(windows)]
fn local_time(secs: i64) -> (i64, u32, u32, u32, u32) {
    let time = secs as libc::time_t;
    // SAFETY: `tm` is plain data, `localtime_s` only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_s(&mut tm, &time) } != 0 {
        return utc_time(secs);
    }
    (
        i64::from(tm.tm_year) + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    )
}

#[cfg(not(any(unix, windows)))]
fn local_time(secs: i64) -> (i64, u32, u32, u32, u32) {
    utc_time(secs)
}

/// Howard Hinnant's `civil_from_days`.
fn utc_time(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
    )
}

#[cfg(test)]
#[path = "tests/home.rs"]
mod tests;
