// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `tpt-workspaces-gui`: a status window and tray icon for the daemon.
//!
//! The window shows the daemon's live state. Closing the window minimizes it;
//! the tray menu brings it back or quits the GUI. The daemon keeps running either
//! way.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;
use tpt_workspaces_core::ipc::{Request, Status};
use tpt_workspaces_gui::{run_action, summary, Connection, SharedState};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Result of the last pause or resume request, shown in the window.
type Feedback = Arc<Mutex<Option<String>>>;

/// A 32×32 solid icon with a light centre, so it is visible in both tray themes.
fn tray_icon_image() -> tray_icon::Icon {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let inner = (8..24).contains(&x) && (8..24).contains(&y);
            let pixel: [u8; 4] = if inner {
                [240, 244, 255, 255]
            } else {
                [59, 130, 246, 255]
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).expect("icon dimensions match the pixel data")
}

/// Runs a pause or resume request on its own thread, so neither the window nor
/// the tray waits for the daemon. The outcome goes to `feedback` and wakes the window.
fn spawn_action(ctx: egui::Context, feedback: Feedback, request: Request, done: &'static str) {
    thread::spawn(move || {
        let message = match run_action(request) {
            Ok(count) => format!("{count} process(es) {done}"),
            Err(e) => format!("Action failed: {e}"),
        };
        if let Ok(mut slot) = feedback.lock() {
            *slot = Some(message);
        }
        ctx.request_repaint();
    });
}

struct App {
    state: SharedState,
    feedback: Feedback,
    _tray: TrayIcon,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, state: SharedState) -> Result<Self, String> {
        let show = MenuItem::new("Show status", true, None);
        let pause = MenuItem::new("Pause background apps", true, None);
        let resume = MenuItem::new("Resume all", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        for item in [&show, &pause, &resume, &quit] {
            menu.append(item).map_err(|e| e.to_string())?;
        }
        let tray = TrayIconBuilder::new()
            .with_tooltip("tpt-workspaces")
            .with_icon(tray_icon_image())
            .with_menu(Box::new(menu))
            .build()
            .map_err(|e| format!("creating tray icon: {e}"))?;

        // Repaint as soon as the poller sees a change, not only on input events.
        let ctx = cc.egui_ctx.clone();
        tpt_workspaces_gui::spawn_poller(state.clone(), POLL_INTERVAL, {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        })
        .map_err(|e| format!("starting poller: {e}"))?;

        let feedback: Feedback = Arc::new(Mutex::new(None));

        // Tray events are handled on their own threads, not in `update`: a hidden
        // window stops receiving repaints, so `update` may not run for a long time.
        let ids = TrayIds {
            show: show.id().clone(),
            pause: pause.id().clone(),
            resume: resume.id().clone(),
            quit: quit.id().clone(),
        };
        let menu_ctx = ctx.clone();
        let menu_feedback = feedback.clone();
        thread::Builder::new()
            .name("tpt-gui-menu".into())
            .spawn(move || {
                for event in MenuEvent::receiver().iter() {
                    handle_menu(&ids, &event.id, &menu_ctx, &menu_feedback);
                }
            })
            .map_err(|e| e.to_string())?;
        thread::Builder::new()
            .name("tpt-gui-tray".into())
            .spawn(move || {
                for event in TrayIconEvent::receiver().iter() {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_window(&ctx);
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(App {
            state,
            feedback,
            _tray: tray,
        })
    }

    fn snapshot(&self) -> Connection {
        self.state
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

struct TrayIds {
    show: MenuId,
    pause: MenuId,
    resume: MenuId,
    quit: MenuId,
}

fn handle_menu(ids: &TrayIds, id: &MenuId, ctx: &egui::Context, feedback: &Feedback) {
    if *id == ids.show {
        show_window(ctx);
    } else if *id == ids.pause {
        spawn_action(
            ctx.clone(),
            feedback.clone(),
            Request::PauseBackground,
            "paused",
        );
    } else if *id == ids.resume {
        spawn_action(ctx.clone(), feedback.clone(), Request::ResumeAll, "resumed");
    } else if *id == ids.quit {
        std::process::exit(0);
    }
}

/// Brings the window back from the tray. Safe to call from any thread.
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Closing the window minimizes it instead of hiding it. A hidden eframe
        // window stops processing commands, so the tray could not bring it back.
        // Only the tray's "Quit" exits.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        let connection = self.snapshot();
        let feedback = self.feedback.lock().map(|f| f.clone()).unwrap_or_default();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("tpt-workspaces");
            ui.label(summary(&connection));
            ui.separator();
            match &connection {
                Connection::Connecting => {
                    ui.spinner();
                }
                Connection::Offline(message) => {
                    ui.colored_label(
                        egui::Color32::GRAY,
                        "The daemon is not running or not answering.",
                    );
                    ui.small(message);
                    ui.add_space(8.0);
                    ui.small("Start it with: tpt-workspaces run");
                }
                Connection::Connected(status) => {
                    show_status(ui, status);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Pause background apps").clicked() {
                            spawn_action(
                                ctx.clone(),
                                self.feedback.clone(),
                                Request::PauseBackground,
                                "paused",
                            );
                        }
                        if ui.button("Resume all").clicked() {
                            spawn_action(
                                ctx.clone(),
                                self.feedback.clone(),
                                Request::ResumeAll,
                                "resumed",
                            );
                        }
                    });
                    if let Some(message) = &feedback {
                        ui.small(message);
                    }
                }
            }
        });

        // Keep the window's state fresh even when the poller has nothing new.
        ctx.request_repaint_after(POLL_INTERVAL);
    }
}

fn show_status(ui: &mut egui::Ui, status: &Status) {
    ui.label(format!("Rules: {}", status.rules));
    ui.add_space(6.0);
    ui.strong("Workspaces");
    if status.workspaces.is_empty() {
        ui.small("None configured.");
    }
    for ws in &status.workspaces {
        let text = format!("{} ({})", ws.name, ws.id);
        if ws.active {
            ui.colored_label(egui::Color32::from_rgb(59, 130, 246), format!("● {text}"));
        } else {
            ui.label(format!("  {text}"));
        }
    }
    ui.add_space(6.0);
    ui.strong("Frozen processes");
    if status.paused.is_empty() {
        ui.small("None.");
    } else {
        for p in &status.paused {
            ui.label(format!("  {} (pid {})", p.executable, p.pid));
        }
    }
}

fn main() -> eframe::Result {
    let state = Arc::new(Mutex::new(Connection::Connecting));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("tpt-workspaces")
            .with_inner_size([420.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tpt-workspaces",
        options,
        Box::new(move |cc| {
            let app = App::new(cc, state)
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(Box::new(app))
        }),
    )
}
