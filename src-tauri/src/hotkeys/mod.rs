//! Global keyboard shortcuts: turn each direction on and off, and push-to-talk, while a game or
//! a call app has the focus and the window is hidden in the tray.
//!
//! # How
//!
//! `tauri-plugin-global-shortcut` registers each shortcut with Windows' `RegisterHotKey`: Windows
//! itself sends the app a message when the combination is pressed. There is no keyboard hook and
//! nothing is injected into other processes, so it is safe with anti-cheat systems. The catch:
//! a registered combination is **taken** — the focused app never sees it — and Windows refuses
//! one another app already registered (or that it reserves). The plugin reports both the press
//! and the release (it polls the key after the press), which push-to-talk needs.
//!
//! Supported: one key (see [`shortcut::KEYS`]) plus any of `Ctrl`, `Alt`, `Shift`, `Win`. Not
//! supported by `RegisterHotKey`: a modifier on its own (just `Ctrl`) and mouse buttons. A mouse's
//! side button can still be used through the mouse's own software, mapped to `F13`–`F24`.
//!
//! # Actions
//!
//! [`HotkeyAction`] is the list of actions (adding one, such as a captions overlay, is a variant,
//! its default and its [`Command`]). Toggles go through the same path as the panel's commands and
//! the tray ([`crate::commands::start_translation`], [`PipelineManager::stop`]), so the tray menu
//! and the panel follow through the usual status events. Push-to-talk sets the [`TalkKey`] the
//! outgoing pipeline's voice gate reads (see [`crate::pipeline::voice`]); while outgoing is not
//! running the key does nothing (the panel shows a hint).
//!
//! # Defaults
//!
//! `Ctrl+Alt+I` / `Ctrl+Alt+O` / `Ctrl+Alt+B` toggle incoming / outgoing / both: games bind bare
//! keys, `Shift` and `Ctrl` combinations, rarely `Ctrl+Alt`, and browsers and Discord use
//! `Ctrl+Shift` ones. On some keyboard layouts `Ctrl+Alt` is `AltGr`, which types characters
//! (Polish `AltGr+O` = ó, Hungarian `AltGr+B` = {): a combination that types a character on the
//! user's layout is never registered ([`layout::types_character`]) and the panel says why.
//! Push-to-talk defaults to `F8`: one key is easy to hold while playing, and it is only taken
//! while the outgoing mode is push-to-talk.
//!
//! # Feedback
//!
//! With `soundCues` on, a short, quiet sound plays in the headphones when a shortcut turns a
//! direction on (rising) or off (falling), or when it could not start (low double tone); see
//! [`cue`].

pub mod cue;
pub mod layout;
pub mod shortcut;

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

pub use shortcut::{Hotkey, ShortcutError};

use crate::error::{AppError, AppResult};
use crate::pipeline::voice::TalkKey;
use crate::pipeline::{Direction, PipelineManager};
use crate::settings::{HotkeySettings, Settings, SettingsStore, TalkMode};

/// Event with the push-to-talk key's state (`{ held }`), on every press and release.
pub const PUSH_TO_TALK_EVENT: &str = "push-to-talk";

/// Something a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAction {
    ToggleIncoming,
    ToggleOutgoing,
    ToggleBoth,
    /// Hold to send the microphone (outgoing in push-to-talk mode).
    PushToTalk,
}

/// What an action does when its shortcut fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Turn these directions on (all of them) or off (when any runs), on press.
    Toggle(&'static [Direction]),
    /// Follow the key: held = talking.
    PushToTalk,
}

impl HotkeyAction {
    /// Every action, in the order the panel lists them.
    pub const ALL: [Self; 4] = [
        Self::ToggleIncoming,
        Self::ToggleOutgoing,
        Self::ToggleBoth,
        Self::PushToTalk,
    ];

    /// The shortcut a new install (or an action missing from the settings) gets.
    pub const fn default_binding(self) -> Option<&'static str> {
        match self {
            Self::ToggleIncoming => Some("Ctrl+Alt+KeyI"),
            Self::ToggleOutgoing => Some("Ctrl+Alt+KeyO"),
            Self::ToggleBoth => Some("Ctrl+Alt+KeyB"),
            Self::PushToTalk => Some("F8"),
        }
    }

    /// What the action does.
    pub const fn command(self) -> Command {
        match self {
            Self::ToggleIncoming => Command::Toggle(&[Direction::Incoming]),
            Self::ToggleOutgoing => Command::Toggle(&[Direction::Outgoing]),
            Self::ToggleBoth => Command::Toggle(&[Direction::Incoming, Direction::Outgoing]),
            Self::PushToTalk => Command::PushToTalk,
        }
    }

    /// `"toggleIncoming"`…: for logs and messages (the settings' key).
    pub const fn name(self) -> &'static str {
        match self {
            Self::ToggleIncoming => "toggleIncoming",
            Self::ToggleOutgoing => "toggleOutgoing",
            Self::ToggleBoth => "toggleBoth",
            Self::PushToTalk => "pushToTalk",
        }
    }
}

/// Why the shortcut settings are not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindingProblem {
    #[error("the {} shortcut \"{shortcut}\" is not valid: {error}", action.name())]
    Invalid {
        action: HotkeyAction,
        shortcut: String,
        error: ShortcutError,
    },
    #[error("{} and {} use the same shortcut {shortcut}", first.name(), second.name())]
    Conflict {
        first: HotkeyAction,
        second: HotkeyAction,
        shortcut: String,
    },
}

/// Every action's shortcut, parsed (`None` = none).
fn parsed(settings: &HotkeySettings) -> Result<Vec<(HotkeyAction, Hotkey)>, BindingProblem> {
    let mut bindings = Vec::new();
    for action in HotkeyAction::ALL {
        let Some(text) = settings.binding(action) else {
            continue;
        };
        let hotkey = Hotkey::parse(text).map_err(|error| BindingProblem::Invalid {
            action,
            shortcut: text.to_owned(),
            error,
        })?;
        bindings.push((action, hotkey));
    }
    Ok(bindings)
}

/// Checks that every shortcut is valid and that no two actions share one (push-to-talk
/// included, even while voice detection is the mode: switching would break otherwise).
pub fn check_bindings(settings: &HotkeySettings) -> Result<(), BindingProblem> {
    let bindings = parsed(settings)?;
    for (index, (first, hotkey)) in bindings.iter().enumerate() {
        if let Some((second, _)) = bindings[index + 1..]
            .iter()
            .find(|(_, other)| other == hotkey)
        {
            return Err(BindingProblem::Conflict {
                first: *first,
                second: *second,
                shortcut: hotkey.to_string(),
            });
        }
    }
    Ok(())
}

/// The shortcuts to register for `settings`: every set one, push-to-talk only in that mode.
/// Shortcuts that do not parse are left out (validation rejects them before a save).
pub fn plan(settings: &Settings) -> Vec<(HotkeyAction, Hotkey)> {
    let push_to_talk = settings.outgoing.talk_mode == TalkMode::PushToTalk;
    HotkeyAction::ALL
        .into_iter()
        .filter(|action| *action != HotkeyAction::PushToTalk || push_to_talk)
        .filter_map(|action| {
            let hotkey = Hotkey::parse(settings.hotkeys.binding(action)?).ok()?;
            Some((action, hotkey))
        })
        .collect()
}

/// Why a shortcut is not registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProblemReason {
    /// Windows refused it: another app has it, or Windows reserves it.
    Unavailable,
    /// It types a character on the keyboard layout (`AltGr`), so it would take that character.
    TypesCharacter,
}

/// A shortcut that is set but not working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyProblem {
    pub action: HotkeyAction,
    pub shortcut: String,
    pub reason: ProblemReason,
}

impl HotkeyProblem {
    fn error(&self) -> AppError {
        match self.reason {
            ProblemReason::Unavailable => AppError::HotkeyUnavailable(self.shortcut.clone()),
            ProblemReason::TypesCharacter => AppError::HotkeyTypesCharacter(self.shortcut.clone()),
        }
    }
}

/// What the panel shows next to the shortcuts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// Shortcuts that are set but not registered.
    pub problems: Vec<HotkeyProblem>,
    /// The shortcuts are off while the panel records a new one.
    pub suspended: bool,
}

/// Registers shortcuts with the system (the plugin; a fake in tests).
pub trait Registrar {
    /// Registers `hotkey`; the error says why Windows refused it.
    fn register(&self, hotkey: &Hotkey) -> Result<(), String>;
    fn unregister(&self, hotkey: &Hotkey);
    /// Whether `hotkey` types a character on the keyboard layout (see [`layout`]).
    fn types_character(&self, hotkey: &Hotkey) -> bool;
}

/// Whether a failure to register is an error for the caller or only a problem to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    /// A save from the panel: a shortcut the user just changed that cannot be registered fails
    /// the save, and the previous shortcuts stay.
    Strict,
    /// Startup (or putting things back): register what can be, report the rest.
    Lenient,
}

#[derive(Default)]
struct State {
    /// What the settings ask for.
    desired: Vec<(HotkeyAction, Hotkey)>,
    /// What is registered with Windows now.
    registered: Vec<(HotkeyAction, Hotkey)>,
    problems: Vec<HotkeyProblem>,
    /// Everything is unregistered while the panel records a shortcut.
    suspended: bool,
}

/// The registered shortcuts and the push-to-talk key, in Tauri managed state.
#[derive(Default)]
pub struct Hotkeys {
    state: Mutex<State>,
    talk_key: TalkKey,
}

impl Hotkeys {
    /// The push-to-talk key the outgoing pipeline follows.
    pub fn talk_key(&self) -> &TalkKey {
        &self.talk_key
    }

    /// Which action `shortcut` belongs to (only while registered).
    pub fn action_for(&self, shortcut: &Shortcut) -> Option<HotkeyAction> {
        self.lock()
            .registered
            .iter()
            .find(|(_, hotkey)| hotkey.shortcut() == *shortcut)
            .map(|(action, _)| *action)
    }

    /// The shortcuts that are set but not working, and whether they are suspended.
    pub fn status(&self) -> HotkeyStatus {
        let state = self.lock();
        HotkeyStatus {
            problems: state.problems.clone(),
            suspended: state.suspended,
        }
    }

    /// Registers `plan` in place of what is registered (see [`Apply`]). Always releases the
    /// push-to-talk key, so a release missed while re-registering never leaves it held.
    pub fn apply(
        &self,
        registrar: &dyn Registrar,
        plan: Vec<(HotkeyAction, Hotkey)>,
        mode: Apply,
    ) -> AppResult<()> {
        let mut state = self.lock();
        if mode == Apply::Strict && plan == state.desired && state.problems.is_empty() {
            return Ok(());
        }
        for (_, hotkey) in std::mem::take(&mut state.registered) {
            registrar.unregister(&hotkey);
        }
        self.talk_key.set_held(false);
        let previous = std::mem::replace(&mut state.desired, plan.clone());
        if state.suspended {
            // Registered when the panel is done recording.
            return Ok(());
        }
        let (registered, problems) = register_all(registrar, &plan);
        let changed = |problem: &&HotkeyProblem| {
            !previous.iter().any(|(action, hotkey)| {
                *action == problem.action && hotkey.to_string() == problem.shortcut
            })
        };
        if mode == Apply::Strict
            && let Some(problem) = problems.iter().find(changed)
        {
            let error = problem.error();
            tracing::warn!(action = problem.action.name(), shortcut = %problem.shortcut, reason = ?problem.reason, "shortcut refused; keeping the previous ones");
            for (_, hotkey) in &registered {
                registrar.unregister(hotkey);
            }
            let (registered, problems) = register_all(registrar, &previous);
            state.desired = previous;
            state.registered = registered;
            state.problems = problems;
            return Err(error);
        }
        for problem in &problems {
            tracing::warn!(action = problem.action.name(), shortcut = %problem.shortcut, reason = ?problem.reason, "shortcut not registered");
        }
        tracing::info!(
            registered = registered.len(),
            problems = problems.len(),
            "shortcuts registered"
        );
        state.registered = registered;
        state.problems = problems;
        Ok(())
    }

    /// Unregisters every shortcut while the panel records one (so the combination reaches the
    /// panel instead of firing), and registers them again afterwards.
    pub fn set_suspended(&self, registrar: &dyn Registrar, suspended: bool) {
        let mut state = self.lock();
        if state.suspended == suspended {
            return;
        }
        state.suspended = suspended;
        if suspended {
            for (_, hotkey) in std::mem::take(&mut state.registered) {
                registrar.unregister(&hotkey);
            }
            self.talk_key.set_held(false);
        } else {
            let (registered, problems) = register_all(registrar, &state.desired);
            state.registered = registered;
            state.problems = problems;
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Registers every shortcut it can; returns what was registered and what was not.
fn register_all(
    registrar: &dyn Registrar,
    plan: &[(HotkeyAction, Hotkey)],
) -> (Vec<(HotkeyAction, Hotkey)>, Vec<HotkeyProblem>) {
    let mut registered = Vec::new();
    let mut problems = Vec::new();
    for &(action, hotkey) in plan {
        let reason = if registrar.types_character(&hotkey) {
            Some(ProblemReason::TypesCharacter)
        } else if let Err(error) = registrar.register(&hotkey) {
            tracing::debug!(%error, shortcut = %hotkey, "RegisterHotKey failed");
            Some(ProblemReason::Unavailable)
        } else {
            None
        };
        match reason {
            None => registered.push((action, hotkey)),
            Some(reason) => problems.push(HotkeyProblem {
                action,
                shortcut: hotkey.to_string(),
                reason,
            }),
        }
    }
    (registered, problems)
}

/// The global-shortcut plugin, as a [`Registrar`].
pub struct PluginRegistrar<'a>(pub &'a AppHandle);

impl Registrar for PluginRegistrar<'_> {
    fn register(&self, hotkey: &Hotkey) -> Result<(), String> {
        self.0
            .global_shortcut()
            .register(hotkey.shortcut())
            .map_err(|error| error.to_string())
    }

    fn unregister(&self, hotkey: &Hotkey) {
        if let Err(error) = self.0.global_shortcut().unregister(hotkey.shortcut()) {
            tracing::debug!(%error, shortcut = %hotkey, "could not unregister a shortcut");
        }
    }

    fn types_character(&self, hotkey: &Hotkey) -> bool {
        layout::types_character(hotkey)
    }
}

/// Registers the shortcuts of `settings` at startup (what Windows refuses is reported).
pub fn register_at_startup(app: &AppHandle, settings: &Settings) {
    let hotkeys = app.state::<Hotkeys>();
    // Lenient never fails.
    let _ = hotkeys.apply(&PluginRegistrar(app), plan(settings), Apply::Lenient);
}

/// Payload of [`PUSH_TO_TALK_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct PushToTalk {
    pub held: bool,
}

/// The plugin's handler: runs the action of a registered shortcut. Never blocks (it runs on
/// the main thread, or the plugin's release-polling thread).
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let Some(hotkeys) = app.try_state::<Hotkeys>() else {
        return;
    };
    let Some(action) = hotkeys.action_for(shortcut) else {
        return;
    };
    match (action.command(), event.state) {
        (Command::PushToTalk, state) => {
            let held = state == ShortcutState::Pressed;
            hotkeys.talk_key().set_held(held);
            tracing::debug!(held, "push-to-talk");
            if let Err(error) = app.emit(PUSH_TO_TALK_EVENT, PushToTalk { held }) {
                tracing::warn!(%error, "could not emit the push-to-talk state");
            }
        }
        (Command::Toggle(directions), ShortcutState::Pressed) => {
            tracing::info!(action = action.name(), "shortcut pressed");
            let app = app.clone();
            tauri::async_runtime::spawn(async move { toggle(&app, directions).await });
        }
        (Command::Toggle(_), ShortcutState::Released) => {}
    }
}

/// Turns `directions` off when any of them runs, else on (all of them), with a sound cue.
pub async fn toggle(app: &AppHandle, directions: &[Direction]) {
    let manager = app.state::<PipelineManager>();
    let running = directions
        .iter()
        .any(|direction| crate::tray::is_active(manager.status(*direction).state));
    if running {
        play_cue(app, cue::Cue::Off);
        futures_util::future::join_all(directions.iter().map(|direction| manager.stop(*direction)))
            .await;
        return;
    }
    play_cue(app, cue::Cue::On);
    let started = futures_util::future::join_all(
        directions
            .iter()
            .map(|direction| crate::commands::start_translation(app, *direction)),
    )
    .await;
    for (direction, result) in directions.iter().zip(started) {
        if let Err(error) = result {
            tracing::warn!(direction = direction.name(), code = error.code(), %error, "could not start from a shortcut");
            play_cue(app, cue::Cue::Error);
        }
    }
}

/// Plays `cue` in the headphones (the incoming output) when sound cues are on.
fn play_cue(app: &AppHandle, which: cue::Cue) {
    let Ok(settings) = app.state::<SettingsStore>().get() else {
        return;
    };
    if settings.hotkeys.sound_cues {
        cue::play(which, settings.incoming.output_device_id);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;
    use crate::settings::HotkeySettings;

    fn hotkeys(bindings: &[(HotkeyAction, Option<&str>)]) -> HotkeySettings {
        let mut settings = HotkeySettings::default();
        for (action, binding) in bindings {
            settings
                .bindings
                .insert(*action, binding.map(str::to_owned));
        }
        settings
    }

    fn hotkey(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap()
    }

    #[test]
    fn defaults_are_valid_and_distinct() {
        assert_eq!(check_bindings(&HotkeySettings::default()), Ok(()));
        let defaults: Vec<_> = HotkeyAction::ALL
            .iter()
            .filter_map(|action| action.default_binding())
            .collect();
        assert_eq!(
            defaults,
            ["Ctrl+Alt+KeyI", "Ctrl+Alt+KeyO", "Ctrl+Alt+KeyB", "F8"]
        );
    }

    #[test]
    fn a_missing_action_uses_its_default_and_null_is_none() {
        let settings = HotkeySettings {
            bindings: BTreeMap::from([(HotkeyAction::ToggleBoth, None)]),
            sound_cues: true,
        };
        assert_eq!(
            settings.binding(HotkeyAction::ToggleIncoming),
            Some("Ctrl+Alt+KeyI")
        );
        assert_eq!(settings.binding(HotkeyAction::ToggleBoth), None);
    }

    #[test]
    fn conflicts_between_actions_are_rejected() {
        let settings = hotkeys(&[(HotkeyAction::ToggleBoth, Some("Alt+Ctrl+KeyI"))]);
        assert_eq!(
            check_bindings(&settings),
            Err(BindingProblem::Conflict {
                first: HotkeyAction::ToggleIncoming,
                second: HotkeyAction::ToggleBoth,
                shortcut: "Ctrl+Alt+KeyI".into(),
            })
        );
        // Push-to-talk counts too, whatever the mode.
        let settings = hotkeys(&[(HotkeyAction::PushToTalk, Some("Ctrl+Alt+KeyO"))]);
        assert!(matches!(
            check_bindings(&settings),
            Err(BindingProblem::Conflict { .. })
        ));
        // Two actions without a shortcut do not conflict.
        let settings = hotkeys(&[
            (HotkeyAction::ToggleIncoming, None),
            (HotkeyAction::ToggleOutgoing, None),
        ]);
        assert_eq!(check_bindings(&settings), Ok(()));
    }

    #[test]
    fn invalid_shortcuts_are_rejected() {
        let settings = hotkeys(&[(HotkeyAction::ToggleIncoming, Some("KeyI"))]);
        assert!(matches!(
            check_bindings(&settings),
            Err(BindingProblem::Invalid {
                action: HotkeyAction::ToggleIncoming,
                error: ShortcutError::NeedsModifier(_),
                ..
            })
        ));
    }

    #[test]
    fn plan_registers_push_to_talk_only_in_that_mode() {
        let mut settings = Settings::default();
        let actions = |settings: &Settings| -> Vec<HotkeyAction> {
            plan(settings)
                .into_iter()
                .map(|(action, _)| action)
                .collect()
        };
        assert_eq!(
            actions(&settings),
            [
                HotkeyAction::ToggleIncoming,
                HotkeyAction::ToggleOutgoing,
                HotkeyAction::ToggleBoth
            ]
        );
        settings.outgoing.talk_mode = TalkMode::PushToTalk;
        settings
            .hotkeys
            .bindings
            .insert(HotkeyAction::ToggleBoth, None);
        assert_eq!(
            actions(&settings),
            [
                HotkeyAction::ToggleIncoming,
                HotkeyAction::ToggleOutgoing,
                HotkeyAction::PushToTalk
            ]
        );
    }

    #[test]
    fn toggles_map_to_their_directions() {
        assert_eq!(
            HotkeyAction::ToggleBoth.command(),
            Command::Toggle(&[Direction::Incoming, Direction::Outgoing])
        );
        assert_eq!(HotkeyAction::PushToTalk.command(), Command::PushToTalk);
        assert_eq!(
            serde_json::to_value(HotkeyAction::ToggleIncoming).unwrap(),
            serde_json::json!("toggleIncoming")
        );
    }

    /// Windows, as far as registering goes: `taken` belong to other apps, `altgr` type a
    /// character.
    #[derive(Default)]
    struct FakeSystem {
        taken: Vec<String>,
        altgr: Vec<String>,
        registered: RefCell<Vec<String>>,
    }

    impl Registrar for FakeSystem {
        fn register(&self, hotkey: &Hotkey) -> Result<(), String> {
            let name = hotkey.to_string();
            let mut registered = self.registered.borrow_mut();
            if self.taken.contains(&name) || registered.contains(&name) {
                return Err("already registered".into());
            }
            registered.push(name);
            Ok(())
        }

        fn unregister(&self, hotkey: &Hotkey) {
            self.registered
                .borrow_mut()
                .retain(|name| *name != hotkey.to_string());
        }

        fn types_character(&self, hotkey: &Hotkey) -> bool {
            self.altgr.contains(&hotkey.to_string())
        }
    }

    fn registered(system: &FakeSystem) -> Vec<String> {
        let mut names = system.registered.borrow().clone();
        names.sort();
        names
    }

    fn plan_of(bindings: &[(HotkeyAction, &str)]) -> Vec<(HotkeyAction, Hotkey)> {
        bindings
            .iter()
            .map(|(action, text)| (*action, hotkey(text)))
            .collect()
    }

    #[test]
    fn lenient_registers_what_it_can_and_reports_the_rest() {
        let system = FakeSystem {
            taken: vec!["Ctrl+Alt+KeyI".into()],
            altgr: vec!["Ctrl+Alt+KeyO".into()],
            ..FakeSystem::default()
        };
        let hotkeys = Hotkeys::default();
        let plan = plan_of(&[
            (HotkeyAction::ToggleIncoming, "Ctrl+Alt+KeyI"),
            (HotkeyAction::ToggleOutgoing, "Ctrl+Alt+KeyO"),
            (HotkeyAction::ToggleBoth, "Ctrl+Alt+KeyB"),
        ]);

        hotkeys.apply(&system, plan, Apply::Lenient).unwrap();

        assert_eq!(registered(&system), ["Ctrl+Alt+KeyB"]);
        let status = hotkeys.status();
        assert_eq!(
            status.problems,
            [
                HotkeyProblem {
                    action: HotkeyAction::ToggleIncoming,
                    shortcut: "Ctrl+Alt+KeyI".into(),
                    reason: ProblemReason::Unavailable,
                },
                HotkeyProblem {
                    action: HotkeyAction::ToggleOutgoing,
                    shortcut: "Ctrl+Alt+KeyO".into(),
                    reason: ProblemReason::TypesCharacter,
                },
            ]
        );
        assert_eq!(
            hotkeys.action_for(&hotkey("Ctrl+Alt+KeyB").shortcut()),
            Some(HotkeyAction::ToggleBoth)
        );
        assert_eq!(
            hotkeys.action_for(&hotkey("Ctrl+Alt+KeyI").shortcut()),
            None
        );
    }

    #[test]
    fn strict_refuses_a_new_shortcut_windows_refuses_and_keeps_the_old_one() {
        let system = FakeSystem {
            taken: vec!["Ctrl+KeyJ".into()],
            ..FakeSystem::default()
        };
        let hotkeys = Hotkeys::default();
        let old = plan_of(&[
            (HotkeyAction::ToggleIncoming, "Ctrl+Alt+KeyI"),
            (HotkeyAction::ToggleOutgoing, "Ctrl+Alt+KeyO"),
        ]);
        hotkeys.apply(&system, old, Apply::Lenient).unwrap();

        let new = plan_of(&[
            (HotkeyAction::ToggleIncoming, "Ctrl+KeyJ"),
            (HotkeyAction::ToggleOutgoing, "Ctrl+Alt+KeyP"),
        ]);
        let error = hotkeys.apply(&system, new, Apply::Strict).unwrap_err();

        assert_eq!(error.code(), "hotkey_unavailable");
        assert!(error.to_string().contains("Ctrl+KeyJ"), "{error}");
        assert_eq!(registered(&system), ["Ctrl+Alt+KeyI", "Ctrl+Alt+KeyO"]);
        assert_eq!(
            hotkeys.action_for(&hotkey("Ctrl+Alt+KeyO").shortcut()),
            Some(HotkeyAction::ToggleOutgoing)
        );
        assert!(hotkeys.status().problems.is_empty());
    }

    #[test]
    fn strict_refuses_a_new_shortcut_that_types_a_character() {
        let system = FakeSystem {
            altgr: vec!["Ctrl+Alt+KeyQ".into()],
            ..FakeSystem::default()
        };
        let hotkeys = Hotkeys::default();
        let error = hotkeys
            .apply(
                &system,
                plan_of(&[(HotkeyAction::ToggleBoth, "Ctrl+Alt+KeyQ")]),
                Apply::Strict,
            )
            .unwrap_err();
        assert_eq!(error.code(), "hotkey_types_character");
        assert!(registered(&system).is_empty());
    }

    #[test]
    fn strict_does_not_fail_a_save_for_an_unchanged_shortcut_that_already_failed() {
        // Another app took Ctrl+Alt+KeyI before LiveDub Voice started.
        let system = FakeSystem {
            taken: vec!["Ctrl+Alt+KeyI".into()],
            ..FakeSystem::default()
        };
        let hotkeys = Hotkeys::default();
        let plan = plan_of(&[
            (HotkeyAction::ToggleIncoming, "Ctrl+Alt+KeyI"),
            (HotkeyAction::ToggleOutgoing, "Ctrl+Alt+KeyO"),
        ]);
        hotkeys
            .apply(&system, plan.clone(), Apply::Lenient)
            .unwrap();

        // The user saves something else: the save goes through, the problem stays reported.
        let mut next = plan;
        next.push((HotkeyAction::ToggleBoth, hotkey("Ctrl+Alt+KeyB")));
        hotkeys.apply(&system, next, Apply::Strict).unwrap();

        assert_eq!(registered(&system), ["Ctrl+Alt+KeyB", "Ctrl+Alt+KeyO"]);
        assert_eq!(hotkeys.status().problems.len(), 1);
    }

    #[test]
    fn suspending_unregisters_everything_until_resumed() {
        let system = FakeSystem::default();
        let hotkeys = Hotkeys::default();
        hotkeys
            .apply(
                &system,
                plan_of(&[(HotkeyAction::PushToTalk, "F8")]),
                Apply::Lenient,
            )
            .unwrap();
        hotkeys.talk_key().set_held(true);

        hotkeys.set_suspended(&system, true);
        assert!(registered(&system).is_empty());
        assert!(hotkeys.status().suspended);
        assert!(!hotkeys.talk_key().is_held(), "a suspended key is released");
        assert_eq!(hotkeys.action_for(&hotkey("F8").shortcut()), None);

        // A save while suspended registers on resume.
        hotkeys
            .apply(
                &system,
                plan_of(&[(HotkeyAction::PushToTalk, "F9")]),
                Apply::Strict,
            )
            .unwrap();
        assert!(registered(&system).is_empty());
        hotkeys.set_suspended(&system, false);
        assert_eq!(registered(&system), ["F9"]);
    }

    #[test]
    fn re_registering_releases_the_push_to_talk_key() {
        let system = FakeSystem::default();
        let hotkeys = Hotkeys::default();
        hotkeys
            .apply(
                &system,
                plan_of(&[(HotkeyAction::PushToTalk, "F8")]),
                Apply::Lenient,
            )
            .unwrap();
        hotkeys.talk_key().set_held(true);
        hotkeys
            .apply(
                &system,
                plan_of(&[(HotkeyAction::PushToTalk, "F7")]),
                Apply::Strict,
            )
            .unwrap();
        assert!(!hotkeys.talk_key().is_held());
        assert_eq!(registered(&system), ["F7"]);
    }
}
