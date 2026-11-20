//! Apps that play audio, for the incoming direction's source picker and for process loopback.
//!
//! [`list_audio_apps`] reads the audio sessions of every active playback device
//! (`IAudioSessionManager2`), takes a snapshot of the running processes and groups the sessions
//! by app: browsers and Electron/WebView2 apps (Chrome, Discord, Teams, …) play audio from a child
//! process, so a session is attributed to the top of its app's process tree, which is also the
//! process that process loopback capture targets (see [`loopback`](super::loopback)).
//!
//! [`resolve_source`] turns the source saved in the settings (an executable name, or "the whole
//! system") into a [`LoopbackTarget`].
//!
//! Grouping is pure ([`group_audio_apps`], [`find_app_root`]) and unit tested; only reading the
//! sessions and the process snapshot talks to Windows.

use std::collections::HashMap;

use serde::Serialize;

use super::loopback::LoopbackTarget;
use crate::error::{AppError, AppResult};
use crate::settings::SourceApp;

/// An app that has audio sessions, as the panel lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApp {
    /// Executable name of the app's root process, e.g. `Discord.exe`. This is what the settings
    /// store (`SourceApp::Process { name }`), compared without case.
    pub process_name: String,
    /// Name to show, e.g. "Google Chrome" (the executable's description when it has one).
    pub display_name: String,
    /// Root of the app's process tree: the process loopback capture targets.
    pub pid: u32,
    /// Processes of the app that own audio sessions (often children of `pid`).
    pub session_pids: Vec<u32>,
    /// Whether the app is playing audio right now (otherwise it has a session but is quiet).
    pub active: bool,
}

/// A running process, from the process snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
    /// Executable file name, e.g. `chrome.exe`.
    pub exe_name: String,
}

/// An audio session of a playback device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// Process that owns the session (0 for the system sounds session).
    pub pid: u32,
    /// Whether the session is playing right now.
    pub active: bool,
    /// Name the app gave the session; often empty, or an indirect string like `@%SystemRoot%…`.
    pub display_name: String,
}

/// Processes that host part of another app (web views, embedded browsers): their audio belongs
/// to the app that started them, even though their executable name differs.
const HELPER_PROCESSES: &[&str] = &[
    "msedgewebview2.exe",
    "cefsharp.browsersubprocess.exe",
    "qtwebengineprocess.exe",
];

/// Processes an app never belongs to, even when they started it: grouping never climbs into them.
const SHELL_PROCESSES: &[&str] = &[
    "explorer.exe",
    "svchost.exe",
    "services.exe",
    "wininit.exe",
    "winlogon.exe",
];

/// Longest parent chain followed (guards against cycles from reused process ids).
const MAX_TREE_DEPTH: usize = 64;

fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn is_listed(name: &str, list: &[&str]) -> bool {
    list.iter().any(|entry| same_name(entry, name))
}

/// Running processes by id.
pub struct ProcessTable {
    by_pid: HashMap<u32, ProcessEntry>,
}

impl ProcessTable {
    pub fn new(processes: impl IntoIterator<Item = ProcessEntry>) -> Self {
        Self {
            by_pid: processes
                .into_iter()
                .map(|process| (process.pid, process))
                .collect(),
        }
    }

    pub fn get(&self, pid: u32) -> Option<&ProcessEntry> {
        self.by_pid.get(&pid)
    }

    fn parent_of(&self, process: &ProcessEntry) -> Option<&ProcessEntry> {
        // Pid 0 is the idle process, whose "parent" is itself.
        if process.parent_pid == 0 || process.parent_pid == process.pid {
            return None;
        }
        self.get(process.parent_pid)
    }

    /// Whether `pid` is `ancestor` or one of its descendants.
    pub fn is_in_tree_of(&self, pid: u32, ancestor: u32) -> bool {
        let mut current = self.get(pid);
        for _ in 0..MAX_TREE_DEPTH {
            match current {
                Some(process) if process.pid == ancestor => return true,
                Some(process) => current = self.parent_of(process),
                None => return false,
            }
        }
        false
    }

    /// Every running process with this executable name (without case).
    fn with_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a ProcessEntry> + 'a {
        self.by_pid
            .values()
            .filter(move |process| same_name(&process.exe_name, name))
    }
}

/// The root of the app `pid` belongs to: climbs to the parent while the parent runs the same
/// executable (Chrome, Discord and Teams classic run every helper as `<app>.exe`) or while the
/// process is a known helper host such as WebView2 (new Teams, WhatsApp). Never climbs into the
/// shell, so apps started from Explorer stay apart.
///
/// Returns `pid` itself when the process is unknown.
pub fn find_app_root(table: &ProcessTable, pid: u32) -> u32 {
    let Some(mut current) = table.get(pid) else {
        return pid;
    };
    for _ in 0..MAX_TREE_DEPTH {
        let Some(parent) = table.parent_of(current) else {
            break;
        };
        let belongs_to_parent = same_name(&current.exe_name, &parent.exe_name)
            || is_listed(&current.exe_name, HELPER_PROCESSES);
        if !belongs_to_parent || is_listed(&parent.exe_name, SHELL_PROCESSES) {
            break;
        }
        current = parent;
    }
    current.pid
}

/// `Discord.exe` → `Discord`.
fn name_without_extension(exe_name: &str) -> &str {
    match exe_name.rsplit_once('.') {
        Some((stem, extension)) if extension.eq_ignore_ascii_case("exe") && !stem.is_empty() => {
            stem
        }
        _ => exe_name,
    }
}

/// A session display name worth showing: set, and not an unresolved indirect string.
fn usable_display_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && !name.starts_with('@')).then_some(name)
}

/// Groups audio sessions into apps.
///
/// Sessions are attributed to their process tree's root ([`find_app_root`]) and merged by the
/// root's executable name, since that is what the settings store. Skipped: the system sounds
/// session (pid 0), sessions of processes that already exited, and LiveDub Voice's own process
/// tree (`own_pid`). `describe` gives a friendly name for a root process (the executable's file
/// description); when it has none the session's own display name or the executable name is used.
///
/// Apps playing right now come first, then by display name.
pub fn group_audio_apps(
    table: &ProcessTable,
    sessions: &[SessionEntry],
    own_pid: u32,
    describe: impl Fn(u32) -> Option<String>,
) -> Vec<AudioApp> {
    struct Group<'a> {
        process_name: &'a str,
        /// Root pid and whether one of its sessions is active, in first-seen order.
        roots: Vec<(u32, bool)>,
        session_pids: Vec<u32>,
        session_name: Option<&'a str>,
    }

    let mut groups: Vec<Group> = Vec::new();
    for session in sessions {
        if session.pid == 0 || table.is_in_tree_of(session.pid, own_pid) {
            continue;
        }
        let root = find_app_root(table, session.pid);
        let Some(root_process) = table.get(root) else {
            continue;
        };
        let index = match groups
            .iter()
            .position(|group| same_name(group.process_name, &root_process.exe_name))
        {
            Some(index) => index,
            None => {
                groups.push(Group {
                    process_name: &root_process.exe_name,
                    roots: Vec::new(),
                    session_pids: Vec::new(),
                    session_name: None,
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[index];
        match group.roots.iter_mut().find(|(pid, _)| *pid == root) {
            Some((_, active)) => *active |= session.active,
            None => group.roots.push((root, session.active)),
        }
        group.session_pids.push(session.pid);
        if group.session_name.is_none() {
            group.session_name = usable_display_name(&session.display_name);
        }
    }

    let mut apps: Vec<AudioApp> = groups
        .into_iter()
        .map(|mut group| {
            // Process loopback captures one tree: prefer the one playing right now.
            let (pid, _) = group
                .roots
                .iter()
                .copied()
                .find(|(_, active)| *active)
                .unwrap_or(group.roots[0]);
            group.session_pids.sort_unstable();
            group.session_pids.dedup();
            let display_name = describe(pid)
                .filter(|name| !name.trim().is_empty())
                .or_else(|| group.session_name.map(str::to_owned))
                .unwrap_or_else(|| name_without_extension(group.process_name).to_owned());
            AudioApp {
                process_name: group.process_name.to_owned(),
                display_name,
                pid,
                session_pids: group.session_pids,
                active: group.roots.iter().any(|(_, active)| *active),
            }
        })
        .collect();
    apps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            })
            .then_with(|| a.pid.cmp(&b.pid))
    });
    apps
}

/// The process tree root to capture for the app saved as `process_name` (an executable name,
/// compared without case), or `None` when it is not running.
///
/// An app that is playing audio wins; otherwise any running tree of that app (process loopback
/// also captures an app that has not opened an audio session yet, from its first sound on). With
/// several trees, the one with the most processes, then the lowest pid, is chosen.
pub fn find_capture_root(
    table: &ProcessTable,
    sessions: &[SessionEntry],
    own_pid: u32,
    process_name: &str,
) -> Option<u32> {
    let playing = group_audio_apps(table, sessions, own_pid, |_| None)
        .into_iter()
        .find(|app| same_name(&app.process_name, process_name));
    if let Some(app) = playing {
        return Some(app.pid);
    }

    let mut trees: HashMap<u32, usize> = HashMap::new();
    for process in table.with_name(process_name) {
        if table.is_in_tree_of(process.pid, own_pid) {
            continue;
        }
        let root = find_app_root(table, process.pid);
        if table
            .get(root)
            .is_some_and(|root| same_name(&root.exe_name, process_name))
        {
            *trees.entry(root).or_default() += 1;
        }
    }
    trees
        .into_iter()
        .max_by(|(pid_a, count_a), (pid_b, count_b)| {
            count_a.cmp(count_b).then_with(|| pid_b.cmp(pid_a))
        })
        .map(|(pid, _)| pid)
}

/// Lists the apps that have audio sessions on any active playback device (see the module docs).
///
/// Blocking (COM and a process snapshot): call it from a blocking thread. It initializes COM on
/// the calling thread for the duration of the call.
pub fn list_audio_apps() -> AppResult<Vec<AudioApp>> {
    let sessions = imp::audio_sessions()?;
    let table = ProcessTable::new(imp::process_snapshot()?);
    let apps = group_audio_apps(&table, &sessions, std::process::id(), imp::describe_process);
    tracing::debug!(
        sessions = sessions.len(),
        apps = apps.len(),
        "listed audio apps"
    );
    Ok(apps)
}

/// A snapshot of the running processes (blocking).
pub fn process_table() -> AppResult<ProcessTable> {
    Ok(ProcessTable::new(imp::process_snapshot()?))
}

/// What to capture for the incoming direction's `source`.
///
/// Blocking, like [`list_audio_apps`]. Fails with [`AppError::SourceAppNotRunning`] when a
/// specific app is chosen and no process of it runs.
pub fn resolve_source(source: &SourceApp) -> AppResult<LoopbackTarget> {
    match source {
        SourceApp::AllExceptSelf => Ok(LoopbackTarget::everything_except_self()),
        SourceApp::Process { name } => {
            let sessions = imp::audio_sessions()?;
            let table = ProcessTable::new(imp::process_snapshot()?);
            let root = find_capture_root(&table, &sessions, std::process::id(), name)
                .ok_or_else(|| AppError::SourceAppNotRunning(name.clone()))?;
            tracing::info!(app = %name, pid = root, "resolved the source app");
            Ok(LoopbackTarget::ProcessTree(root))
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use wasapi::{DeviceEnumerator, Direction, SessionState};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::core::{HSTRING, PWSTR};

    use super::{ProcessEntry, SessionEntry};
    use crate::audio::stream::audio_error;
    use crate::audio::stream::win::{ComGuard, hresult_of, map_error};
    use crate::error::AppResult;

    const LIST: &str = "could not list the apps playing audio";

    /// Sessions of every active playback device. A device or session that fails to read is
    /// skipped (it may have just gone away) instead of failing the whole list.
    pub(super) fn audio_sessions() -> AppResult<Vec<SessionEntry>> {
        let _com = ComGuard::new()?;
        let enumerator = DeviceEnumerator::new().map_err(|e| map_error(LIST, e))?;
        let devices = enumerator
            .get_device_collection(Direction::Render)
            .map_err(|e| map_error(LIST, e))?;
        let count = devices.get_nbr_devices().map_err(|e| map_error(LIST, e))?;
        let mut sessions = Vec::new();
        for index in 0..count {
            let read = (|| -> Result<(), wasapi::WasapiError> {
                let device = devices.get_device_at_index(index)?;
                let manager = device.get_iaudiosessionmanager()?;
                let enumerator = manager.get_audiosessionenumerator()?;
                for session_index in 0..enumerator.get_count()? {
                    let session = match enumerator.get_session(session_index) {
                        Ok(session) => session,
                        Err(error) => {
                            tracing::debug!(%error, "skipping an audio session");
                            continue;
                        }
                    };
                    let state = session.get_state()?;
                    if state == SessionState::Expired {
                        continue;
                    }
                    sessions.push(SessionEntry {
                        pid: session.get_process_id()?,
                        active: state == SessionState::Active,
                        display_name: session.get_display_name().unwrap_or_default(),
                    });
                }
                Ok(())
            })();
            if let Err(error) = read {
                tracing::warn!(%error, hresult = ?hresult_of(&error), index, "skipping the sessions of a playback device");
            }
        }
        Ok(sessions)
    }

    /// Closes a Win32 handle on drop.
    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by us and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn wide_to_string(wide: &[u16]) -> String {
        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..len])
    }

    /// Every running process (id, parent id and executable name).
    pub(super) fn process_snapshot() -> AppResult<Vec<ProcessEntry>> {
        // SAFETY: plain Win32 calls; the entry is sized as the API requires and the snapshot
        // handle is closed by `OwnedHandle`.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map(OwnedHandle)
                .map_err(|e| audio_error(LIST, Some(e.code().0), &e))?;
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut processes = Vec::new();
            let mut next = Process32FirstW(snapshot.0, &mut entry);
            while next.is_ok() {
                processes.push(ProcessEntry {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    exe_name: wide_to_string(&entry.szExeFile),
                });
                next = Process32NextW(snapshot.0, &mut entry);
            }
            Ok(processes)
        }
    }

    /// Full path of a process's executable, when we may query it.
    fn executable_path(pid: u32) -> Option<String> {
        // SAFETY: the buffer and its length are passed together; the handle is closed on drop.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .ok()
                .map(OwnedHandle)?;
            let mut buffer = [0_u16; 1024];
            let mut len = buffer.len() as u32;
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
            .ok()?;
            Some(String::from_utf16_lossy(&buffer[..len as usize]))
        }
    }

    /// Reads a string value of the version resource (`value` like `FileDescription`).
    fn version_string(path: &str, value: &str) -> Option<String> {
        let path = HSTRING::from(path);
        // SAFETY: `data` is sized by GetFileVersionInfoSizeW and outlives every pointer
        // VerQueryValueW returns into it; lengths come from the API.
        unsafe {
            let size = GetFileVersionInfoSizeW(&path, None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0_u8; size as usize];
            GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;

            let query = |sub_block: &str| -> Option<(*const u8, usize)> {
                let mut pointer: *mut c_void = std::ptr::null_mut();
                let mut len = 0_u32;
                let found = VerQueryValueW(
                    data.as_ptr().cast(),
                    &HSTRING::from(sub_block),
                    &mut pointer,
                    &mut len,
                );
                (found.as_bool() && !pointer.is_null() && len > 0)
                    .then_some((pointer as *const u8, len as usize))
            };

            // Language/code page pairs; fall back to US English, Unicode.
            let mut translations = vec![(0x0409_u16, 0x04b0_u16)];
            if let Some((pointer, len)) = query("\\VarFileInfo\\Translation") {
                let pairs = std::slice::from_raw_parts(pointer.cast::<u16>(), len / 2);
                translations.splice(
                    0..0,
                    pairs
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[language, code_page]| (language, code_page)),
                );
            }
            translations.into_iter().find_map(|(language, code_page)| {
                let (pointer, len) = query(&format!(
                    "\\StringFileInfo\\{language:04x}{code_page:04x}\\{value}"
                ))?;
                // `len` counts UTF-16 characters, including the terminator.
                let text = wide_to_string(std::slice::from_raw_parts(pointer.cast::<u16>(), len));
                let text = text.trim();
                (!text.is_empty()).then(|| text.to_owned())
            })
        }
    }

    /// The executable's description (e.g. "Google Chrome"), when it has one and we may read it.
    pub(super) fn describe_process(pid: u32) -> Option<String> {
        version_string(&executable_path(pid)?, "FileDescription")
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{ProcessEntry, SessionEntry};
    use crate::error::AppResult;

    /// LiveDub Voice only supports Windows; elsewhere no app plays audio we could capture.
    pub(super) fn audio_sessions() -> AppResult<Vec<SessionEntry>> {
        Ok(Vec::new())
    }

    pub(super) fn process_snapshot() -> AppResult<Vec<ProcessEntry>> {
        Ok(Vec::new())
    }

    pub(super) fn describe_process(_: u32) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 9000;

    fn process(pid: u32, parent_pid: u32, exe_name: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            exe_name: exe_name.into(),
        }
    }

    fn session(pid: u32, active: bool) -> SessionEntry {
        SessionEntry {
            pid,
            active,
            display_name: String::new(),
        }
    }

    /// A desktop with Chrome, Discord, new Teams (WebView2), Spotify, a Steam game and LiveDub
    /// Voice itself with its own WebView2.
    fn desktop() -> ProcessTable {
        ProcessTable::new([
            process(4, 0, "System"),
            process(700, 600, "services.exe"),
            process(800, 700, "svchost.exe"),
            process(1000, 900, "explorer.exe"),
            // Chrome: browser process, renderer and audio service.
            process(1100, 1000, "chrome.exe"),
            process(1101, 1100, "chrome.exe"),
            process(1102, 1100, "chrome.exe"),
            // Discord: started by its updater, which exited (parent 1199 is gone).
            process(1200, 1199, "Discord.exe"),
            process(1201, 1200, "Discord.exe"),
            // New Teams: audio from a WebView2 child of a WebView2 child.
            process(1300, 1000, "ms-teams.exe"),
            process(1301, 1300, "msedgewebview2.exe"),
            process(1302, 1301, "msedgewebview2.exe"),
            process(1400, 1000, "Spotify.exe"),
            // A game started by Steam: its own app, not Steam's.
            process(1500, 1000, "steam.exe"),
            process(1501, 1500, "game.exe"),
            // A WebView2 started by a service: never grouped into svchost.
            process(1600, 800, "msedgewebview2.exe"),
            // LiveDub Voice and its panel.
            process(OWN, 1000, "livedub-voice.exe"),
            process(9001, OWN, "msedgewebview2.exe"),
            process(9002, 9001, "msedgewebview2.exe"),
        ])
    }

    fn names(apps: &[AudioApp]) -> Vec<&str> {
        apps.iter().map(|app| app.process_name.as_str()).collect()
    }

    #[test]
    fn helper_processes_belong_to_their_app_root() {
        let table = desktop();

        assert_eq!(find_app_root(&table, 1102), 1100);
        assert_eq!(find_app_root(&table, 1201), 1200);
        assert_eq!(find_app_root(&table, 1302), 1300);
        assert_eq!(find_app_root(&table, 1400), 1400);
    }

    #[test]
    fn grouping_stops_at_other_apps_and_the_shell() {
        let table = desktop();

        // A game is not Steam.
        assert_eq!(find_app_root(&table, 1501), 1501);
        // A helper never climbs into a system service.
        assert_eq!(find_app_root(&table, 1600), 1600);
        // Unknown processes are their own root.
        assert_eq!(find_app_root(&table, 4242), 4242);
    }

    #[test]
    fn grouping_survives_parent_cycles() {
        // Reused ids can make a process its own grandparent.
        let table = ProcessTable::new([process(10, 11, "a.exe"), process(11, 10, "a.exe")]);

        let root = find_app_root(&table, 10);

        assert!(root == 10 || root == 11);
        assert!(!table.is_in_tree_of(10, 99));
    }

    #[test]
    fn sessions_are_grouped_by_app() {
        let table = desktop();
        let sessions = [
            session(1102, true),
            session(1101, false),
            session(1201, false),
            session(1302, true),
            session(1400, false),
        ];

        let apps = group_audio_apps(&table, &sessions, OWN, |_| None);

        assert_eq!(
            names(&apps),
            ["chrome.exe", "ms-teams.exe", "Discord.exe", "Spotify.exe"]
        );
        let chrome = &apps[0];
        assert_eq!(chrome.pid, 1100);
        assert_eq!(chrome.session_pids, [1101, 1102]);
        assert!(chrome.active);
        let teams = &apps[1];
        assert_eq!(
            (teams.pid, teams.session_pids.as_slice()),
            (1300, &[1302][..])
        );
        assert!(!apps[2].active);
    }

    #[test]
    fn own_tree_and_system_sounds_are_skipped() {
        let table = desktop();
        let sessions = [
            session(0, true),
            session(OWN, true),
            session(9002, true),
            session(1400, true),
            // Exited since the sessions were read.
            session(31337, true),
        ];

        let apps = group_audio_apps(&table, &sessions, OWN, |_| None);

        assert_eq!(names(&apps), ["Spotify.exe"]);
    }

    #[test]
    fn display_name_prefers_the_description_then_the_session_name() {
        let table = desktop();
        let mut discord = session(1201, true);
        discord.display_name = "Discord Voice".into();
        let mut spotify = session(1400, true);
        spotify.display_name = "@%SystemRoot%\\System32\\AudioSrv.Dll,-202".into();
        let sessions = [session(1102, true), discord, spotify];

        let apps = group_audio_apps(&table, &sessions, OWN, |pid| {
            (pid == 1100).then(|| "Google Chrome".to_owned())
        });

        let shown: Vec<_> = apps.iter().map(|app| app.display_name.as_str()).collect();
        assert_eq!(shown, ["Discord Voice", "Google Chrome", "Spotify"]);
    }

    #[test]
    fn active_apps_come_first_then_by_name() {
        let table = desktop();
        let sessions = [
            session(1400, false),
            session(1201, false),
            session(1501, true),
        ];

        let apps = group_audio_apps(&table, &sessions, OWN, |_| None);

        assert_eq!(names(&apps), ["game.exe", "Discord.exe", "Spotify.exe"]);
    }

    #[test]
    fn separate_trees_of_one_app_merge_into_the_playing_one() {
        let table = ProcessTable::new([
            process(1000, 900, "explorer.exe"),
            process(20, 1000, "chrome.exe"),
            process(21, 20, "chrome.exe"),
            process(30, 1000, "Chrome.exe"),
            process(31, 30, "chrome.exe"),
        ]);

        let apps = group_audio_apps(
            &table,
            &[session(21, false), session(31, true)],
            OWN,
            |_| None,
        );

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pid, 30);
        assert_eq!(apps[0].session_pids, [21, 31]);
    }

    #[test]
    fn capture_root_prefers_the_app_playing_audio() {
        let table = desktop();

        assert_eq!(
            find_capture_root(&table, &[session(1302, true)], OWN, "MS-TEAMS.EXE"),
            Some(1300)
        );
    }

    #[test]
    fn capture_root_finds_apps_without_sessions() {
        let table = desktop();

        // Discord is running but quiet: capture its tree anyway.
        assert_eq!(
            find_capture_root(&table, &[], OWN, "discord.exe"),
            Some(1200)
        );
        assert_eq!(find_capture_root(&table, &[], OWN, "zoom.exe"), None);
        // LiveDub Voice never captures itself.
        assert_eq!(
            find_capture_root(&table, &[], OWN, "livedub-voice.exe"),
            None
        );
    }

    #[test]
    fn capture_root_picks_the_biggest_tree() {
        let table = ProcessTable::new([
            process(1000, 900, "explorer.exe"),
            process(50, 1000, "app.exe"),
            process(60, 1000, "app.exe"),
            process(61, 60, "app.exe"),
        ]);

        assert_eq!(find_capture_root(&table, &[], OWN, "app.exe"), Some(60));
    }

    #[test]
    fn audio_app_serializes_to_the_panel_shape() {
        let app = AudioApp {
            process_name: "Discord.exe".into(),
            display_name: "Discord".into(),
            pid: 1200,
            session_pids: vec![1201],
            active: true,
        };

        assert_eq!(
            serde_json::to_value(app).unwrap(),
            serde_json::json!({
                "processName": "Discord.exe",
                "displayName": "Discord",
                "pid": 1200,
                "sessionPids": [1201],
                "active": true
            })
        );
    }

    #[test]
    fn resolving_the_whole_system_excludes_our_own_tree() {
        assert_eq!(
            resolve_source(&SourceApp::AllExceptSelf).unwrap(),
            LoopbackTarget::ExcludeProcessTree(std::process::id())
        );
    }

    #[cfg(windows)]
    #[test]
    fn resolving_an_app_that_is_not_running_fails() {
        let error = resolve_source(&SourceApp::Process {
            name: "livedub-no-such-app.exe".into(),
        })
        .unwrap_err();

        assert_eq!(error.code(), "source_app_not_running");
    }

    /// Prints the apps playing audio on this machine. Play something (a YouTube video, a Discord
    /// call) first. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml list_audio_apps_on_this_machine -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore = "reads this machine's audio sessions"]
    fn list_audio_apps_on_this_machine() {
        for app in list_audio_apps().unwrap() {
            println!(
                "{:<28} {:<24} pid {:<6} sessions {:?} {}",
                app.display_name,
                app.process_name,
                app.pid,
                app.session_pids,
                if app.active { "playing" } else { "quiet" }
            );
        }
    }

    #[test]
    fn executable_names_lose_their_extension() {
        assert_eq!(name_without_extension("Discord.exe"), "Discord");
        assert_eq!(name_without_extension("GAME.EXE"), "GAME");
        assert_eq!(name_without_extension("weird"), "weird");
        assert_eq!(name_without_extension(".exe"), ".exe");
    }
}
