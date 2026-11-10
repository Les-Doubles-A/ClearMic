//! Icône de la zone de notification : la fenêtre minimisée s'y cache au lieu
//! de rester dans la barre des tâches.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const ID_OPEN: &str = "open";
const ID_QUIT: &str = "quit";

pub struct Tray {
    _icon: TrayIcon,
    hidden: Arc<AtomicBool>,
}

impl Tray {
    /// À appeler depuis le thread de la boucle d'événements (création de l'app).
    pub fn new(ctx: &egui::Context) -> Result<Self, String> {
        let menu = Menu::new();
        menu.append(&MenuItem::with_id(ID_OPEN, "Ouvrir ClearMic", true, None)).map_err(|e| e.to_string())?;
        menu.append(&MenuItem::with_id(ID_QUIT, "Quitter", true, None)).map_err(|e| e.to_string())?;

        let icon = TrayIconBuilder::new()
            .with_tooltip("ClearMic")
            .with_icon(make_icon()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;

        let hidden = Arc::new(AtomicBool::new(false));

        let (c, h) = (ctx.clone(), hidden.clone());
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => show(&c, &h),
            _ => {}
        }));

        let (c, h) = (ctx.clone(), hidden.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == ID_OPEN {
                show(&c, &h);
            } else if event.id == ID_QUIT {
                // La fenêtre doit être visible pour traiter la commande de fermeture.
                show(&c, &h);
                c.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }));

        Ok(Self { _icon: icon, hidden })
    }

    /// Cache la fenêtre dans la zone de notification si elle vient d'être minimisée.
    pub fn hide_if_minimized(&self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        if minimized && !self.hidden.swap(true, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
}

fn show(ctx: &egui::Context, hidden: &AtomicBool) {
    hidden.store(false, Ordering::SeqCst);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Disque vert avec un point blanc central, généré en mémoire (pas de fichier image).
fn make_icon() -> Result<Icon, String> {
    const N: u32 = 32;
    let c = (N as f32 - 1.0) / 2.0;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let px = if d <= 6.0 {
                [255, 255, 255, 255]
            } else if d <= 15.0 {
                [40, 170, 90, 255]
            } else {
                [0, 0, 0, 0]
            };
            rgba.extend_from_slice(&px);
        }
    }
    Icon::from_rgba(rgba, N, N).map_err(|e| e.to_string())
}
