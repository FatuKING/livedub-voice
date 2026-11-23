//! The system tray icon and what closing the window does.
//!
//! Like Discord, LiveDub Voice keeps running in the tray when its window is closed: the
//! translations go on (a call or a game does not stop because the panel is out of the way).
//!
//! - **Closing the window** ([`close_action`]): hides it to the tray, unless the user chose
//!   "Closing the window quits the app" (`closeQuitsApp`) or there is no tray icon. The first
//!   time, the window stays open and the panel shows a one-off notice explaining it (the
//!   `close-to-tray-notice` event); the user then hides it ([`hide_to_tray`]) or quits.
//! - **Tray menu:** open LiveDub Voice, start/stop each direction (a check mark and the current
//!   state), quit. A left click on the icon opens or focuses the window.
//! - **Quit** (tray menu or Settings) exits the app, which goes through the usual graceful
//!   shutdown (`commands::shut_down_pipelines`: `session.close`, threads joined, cable
//!   released, ducked apps restored).
//!
//! The menu is built here, but its strings live in the panel's dictionaries like every other
//! string: the panel sends them in its language ([`TrayLabels`], `set_tray_labels`) when it
//! starts and whenever the app language changes. Until then the menu is in English.
//!
//! Menu updates never block the caller: pipeline events arrive on audio threads and async
//! tasks, possibly while the main thread waits for them at exit, so the update is queued on the
//! main thread ([`refresh`]).

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Deserialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::pipeline::{Direction, PipelineManager, PipelineState};
use crate::settings::Settings;

/// Label of the main window (the only one).
pub const MAIN_WINDOW: &str = "main";

/// Event sent to the panel the first time the window is closed (no payload): it explains that
/// the app keeps running in the tray.
pub const CLOSE_TO_TRAY_NOTICE_EVENT: &str = "close-to-tray-notice";

const TRAY_ID: &str = "livedub";
const OPEN_ID: &str = "open";
const INCOMING_ID: &str = "incoming";
const OUTGOING_ID: &str = "outgoing";
const QUIT_ID: &str = "quit";

/// What closing the window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Close it: the app exits (gracefully).
    Quit,
    /// Hide it; the app keeps running in the tray.
    HideToTray,
    /// Keep it open and let the panel explain the tray first (once).
    ExplainTray,
}

/// What closing the window does, from the settings and whether the tray icon exists.
pub fn close_action(settings: &Settings, has_tray: bool) -> CloseAction {
    if settings.close_quits_app || !has_tray {
        CloseAction::Quit
    } else if settings.tray_notice_shown {
        CloseAction::HideToTray
    } else {
        CloseAction::ExplainTray
    }
}

/// The tray's strings, in the panel's language (from `src/i18n`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayLabels {
    /// "Open LiveDub Voice".
    pub open: String,
    /// "Quit LiveDub Voice".
    pub quit: String,
    /// "Incoming (others → you)".
    pub incoming: String,
    /// "Outgoing (you → others)".
    pub outgoing: String,
    /// The state of a direction, next to its name.
    pub states: StateLabels,
}

/// A direction's state as the tray shows it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateLabels {
    pub idle: String,
    pub connecting: String,
    pub live: String,
    pub reconnecting: String,
    pub stopping: String,
    pub error: String,
}

impl StateLabels {
    fn get(&self, state: PipelineState) -> &str {
        match state {
            PipelineState::Idle => &self.idle,
            PipelineState::Connecting => &self.connecting,
            PipelineState::Live => &self.live,
            PipelineState::Reconnecting => &self.reconnecting,
            PipelineState::Stopping => &self.stopping,
            PipelineState::Error => &self.error,
        }
    }
}

impl Default for TrayLabels {
    /// English, until the panel sends its own.
    fn default() -> Self {
        Self {
            open: "Open LiveDub Voice".into(),
            quit: "Quit LiveDub Voice".into(),
            incoming: "Incoming (others → you)".into(),
            outgoing: "Outgoing (you → others)".into(),
            states: StateLabels {
                idle: "Off".into(),
                connecting: "Connecting…".into(),
                live: "Live".into(),
                reconnecting: "Reconnecting…".into(),
                stopping: "Finishing…".into(),
                error: "Error".into(),
            },
        }
    }
}

/// Whether a direction holds a session (or is about to): the tray item is checked, and a click
/// stops it.
pub fn is_active(state: PipelineState) -> bool {
    matches!(
        state,
        PipelineState::Connecting
            | PipelineState::Live
            | PipelineState::Reconnecting
            | PipelineState::Stopping
    )
}

/// A direction's menu text: "Incoming (others → you): Live".
pub fn item_text(labels: &TrayLabels, direction: Direction, state: PipelineState) -> String {
    let name = match direction {
        Direction::Incoming => &labels.incoming,
        Direction::Outgoing => &labels.outgoing,
    };
    format!("{name}: {}", labels.states.get(state))
}

/// The icon's tooltip: the app name and both directions' states, one per line.
pub fn tooltip(labels: &TrayLabels, incoming: PipelineState, outgoing: PipelineState) -> String {
    format!(
        "LiveDub Voice\n{}\n{}",
        item_text(labels, Direction::Incoming, incoming),
        item_text(labels, Direction::Outgoing, outgoing)
    )
}

struct TrayMenu {
    icon: TrayIcon,
    open: MenuItem<Wry>,
    incoming: CheckMenuItem<Wry>,
    outgoing: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// The tray icon and its strings, in Tauri managed state.
pub struct Tray {
    labels: Mutex<TrayLabels>,
    menu: Mutex<Option<TrayMenu>>,
}

impl Default for Tray {
    fn default() -> Self {
        Self {
            labels: Mutex::new(TrayLabels::default()),
            menu: Mutex::new(None),
        }
    }
}

impl Tray {
    /// Whether the icon was created.
    pub fn exists(&self) -> bool {
        lock(&self.menu).is_some()
    }
}

/// Creates the tray icon (the app icon at tray size) and its menu. Call once, at startup.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<Tray>();
    let labels = lock(&state.labels).clone();
    let open = MenuItem::with_id(app, OPEN_ID, &labels.open, true, None::<&str>)?;
    let incoming = CheckMenuItem::with_id(
        app,
        INCOMING_ID,
        item_text(&labels, Direction::Incoming, PipelineState::Idle),
        true,
        false,
        None::<&str>,
    )?;
    let outgoing = CheckMenuItem::with_id(
        app,
        OUTGOING_ID,
        item_text(&labels, Direction::Outgoing, PipelineState::Idle),
        true,
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, QUIT_ID, &labels.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &incoming,
            &outgoing,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(&labels, PipelineState::Idle, PipelineState::Idle))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|icon, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(icon.app_handle());
            }
        });
    if let Some(icon) = tray_image(app) {
        builder = builder.icon(icon);
    }
    let icon = builder.build(app)?;
    tracing::info!("tray icon ready");
    *lock(&state.menu) = Some(TrayMenu {
        icon,
        open,
        incoming,
        outgoing,
        quit,
    });
    Ok(())
}

/// The app icon at the size Windows draws tray icons (16 px at 100 % scaling, 24 px at 150 %…).
///
/// It comes from the icon embedded in the executable, whose 16, 20 and 24 px entries are a
/// simplified drawing made for that size (`src-tauri/icons/icon-small.svg`); the default window
/// icon is the 32 px one, which Windows would shrink and blur. Falls back to the window icon.
fn tray_image(app: &AppHandle) -> Option<tauri::image::Image<'static>> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
        // SAFETY: `GetSystemMetrics` only reads a system value; it returns 0 on failure.
        let size = match unsafe { GetSystemMetrics(SM_CXSMICON) } {
            n if n > 0 => n.unsigned_abs(),
            _ => 16,
        };
        match tauri::image::Image::from_app_icon_resource(size) {
            Ok(icon) => return Some(icon),
            Err(error) => tracing::warn!(%error, "no tray-sized app icon, using the window icon"),
        }
    }
    app.default_window_icon()
        .map(|icon| icon.clone().to_owned())
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        OPEN_ID => show_window(app),
        INCOMING_ID => toggle(app, Direction::Incoming),
        OUTGOING_ID => toggle(app, Direction::Outgoing),
        QUIT_ID => {
            tracing::info!("quit from the tray");
            app.exit(0);
        }
        _ => {}
    }
}

/// Starts or stops `direction` from the tray. A start that fails opens the window, where the
/// error (or the spending cap) is shown.
fn toggle(app: &AppHandle, direction: Direction) {
    let state = app.state::<PipelineManager>().status(direction).state;
    // The check mark toggles itself on click: put it back until the status says otherwise.
    refresh(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if is_active(state) {
            app.state::<PipelineManager>().stop(direction).await;
        } else if let Err(error) = crate::commands::start_translation(&app, direction).await {
            tracing::warn!(direction = direction.name(), code = error.code(), %error, "could not start from the tray");
            show_window(&app);
        }
    });
}

/// Shows, restores and focuses the main window.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let shown = window
            .show()
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus());
        if let Err(error) = shown {
            tracing::warn!(%error, "could not show the window");
        }
    }
}

/// Hides the main window (the app keeps running in the tray).
pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW)
        && let Err(error) = window.hide()
    {
        tracing::warn!(%error, "could not hide the window");
    }
}

/// Replaces the tray's strings (the panel's language changed) and updates the menu.
pub fn set_labels(app: &AppHandle, labels: TrayLabels) {
    *lock(&app.state::<Tray>().labels) = labels;
    refresh(app);
}

/// Updates the menu and tooltip from both directions' status. Never blocks: the update runs on
/// the main thread when it is free.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        let tray = handle.state::<Tray>();
        let menu = lock(&tray.menu);
        let Some(menu) = menu.as_ref() else {
            return;
        };
        let labels = lock(&tray.labels).clone();
        let pipelines = handle.state::<PipelineManager>();
        let incoming = pipelines.status(Direction::Incoming).state;
        let outgoing = pipelines.status(Direction::Outgoing).state;
        let updated = menu
            .open
            .set_text(&labels.open)
            .and_then(|()| menu.quit.set_text(&labels.quit))
            .and_then(|()| {
                menu.incoming
                    .set_text(item_text(&labels, Direction::Incoming, incoming))
            })
            .and_then(|()| menu.incoming.set_checked(is_active(incoming)))
            .and_then(|()| {
                menu.outgoing
                    .set_text(item_text(&labels, Direction::Outgoing, outgoing))
            })
            .and_then(|()| menu.outgoing.set_checked(is_active(outgoing)))
            .and_then(|()| {
                menu.icon
                    .set_tooltip(Some(tooltip(&labels, incoming, outgoing)))
            });
        if let Err(error) = updated {
            tracing::warn!(%error, "could not update the tray menu");
        }
    });
    if let Err(error) = queued {
        tracing::debug!(%error, "could not queue a tray update");
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(close_quits_app: bool, tray_notice_shown: bool) -> Settings {
        Settings {
            close_quits_app,
            tray_notice_shown,
            ..Settings::default()
        }
    }

    #[test]
    fn closing_explains_the_tray_the_first_time_then_hides() {
        assert_eq!(
            close_action(&settings(false, false), true),
            CloseAction::ExplainTray
        );
        assert_eq!(
            close_action(&settings(false, true), true),
            CloseAction::HideToTray
        );
    }

    #[test]
    fn closing_quits_when_the_user_chose_it() {
        assert_eq!(
            close_action(&settings(true, false), true),
            CloseAction::Quit
        );
        assert_eq!(close_action(&settings(true, true), true), CloseAction::Quit);
    }

    #[test]
    fn closing_quits_without_a_tray_icon() {
        // Hiding the only way back to the app would strand it.
        assert_eq!(
            close_action(&settings(false, true), false),
            CloseAction::Quit
        );
        assert_eq!(
            close_action(&settings(false, false), false),
            CloseAction::Quit
        );
    }

    #[test]
    fn the_default_settings_hide_to_the_tray() {
        assert_eq!(
            close_action(&Settings::default(), true),
            CloseAction::ExplainTray
        );
    }

    #[test]
    fn item_text_shows_the_direction_and_its_state() {
        let labels = TrayLabels::default();
        assert_eq!(
            item_text(&labels, Direction::Incoming, PipelineState::Live),
            "Incoming (others → you): Live"
        );
        assert_eq!(
            item_text(&labels, Direction::Outgoing, PipelineState::Idle),
            "Outgoing (you → others): Off"
        );
        assert_eq!(
            tooltip(&labels, PipelineState::Reconnecting, PipelineState::Error),
            "LiveDub Voice\nIncoming (others → you): Reconnecting…\nOutgoing (you → others): Error"
        );
    }

    #[test]
    fn active_states_are_checked() {
        for state in [
            PipelineState::Connecting,
            PipelineState::Live,
            PipelineState::Reconnecting,
            PipelineState::Stopping,
        ] {
            assert!(is_active(state), "{state:?}");
        }
        assert!(!is_active(PipelineState::Idle));
        assert!(!is_active(PipelineState::Error));
    }

    #[test]
    fn labels_come_from_the_panel_in_its_shape() {
        let labels: TrayLabels = serde_json::from_value(serde_json::json!({
            "open": "Abrir LiveDub Voice",
            "quit": "Salir de LiveDub Voice",
            "incoming": "Entrante",
            "outgoing": "Saliente",
            "states": {
                "idle": "Apagada",
                "connecting": "Conectando…",
                "live": "En vivo",
                "reconnecting": "Reconectando…",
                "stopping": "Terminando…",
                "error": "Error"
            }
        }))
        .unwrap();
        assert_eq!(
            item_text(&labels, Direction::Incoming, PipelineState::Live),
            "Entrante: En vivo"
        );
    }
}
