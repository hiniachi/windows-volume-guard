#![allow(non_snake_case)]

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    ptr, thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use windows::{
    core::{implement, Interface, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            eRender, IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2,
            IAudioSessionNotification, IAudioSessionNotification_Impl, IMMDevice,
            IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_MULTITHREADED,
            },
            Threading::{
                CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
                EVENT_MODIFY_STATE,
            },
        },
    },
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::{policy::desired_volume, Settings};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "WindowsVolumeGuard";
const APP_DIR: &str = "WindowsVolumeGuard";
const EXE_NAME: &str = "windows-volume-guard.exe";
const LAUNCHER_NAME: &str = "launch-hidden.vbs";
const MUTEX_NAME: PCWSTR = windows::core::w!("Local\\WindowsVolumeGuard.SingleInstance");
const STOP_EVENT_NAME: PCWSTR = windows::core::w!("Local\\WindowsVolumeGuard.Stop");

struct ComGuard;

impl ComGuard {
    fn initialize() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .context("could not initialize COM in multithreaded mode")?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct WinHandle(HANDLE);

impl Drop for WinHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[implement(IAudioSessionNotification)]
struct SessionNotification {
    settings: Settings,
}

impl IAudioSessionNotification_Impl for SessionNotification_Impl {
    fn OnSessionCreated(
        &self,
        new_session: Option<&IAudioSessionControl>,
    ) -> windows::core::Result<()> {
        if let Some(session) = new_session {
            // Core Audio callbacks must return quickly. A volume read and an
            // optional volume write are bounded, in-process COM calls.
            let _ = adjust_session(session, &self.settings);
        }
        Ok(())
    }
}

struct EndpointWatch {
    manager: IAudioSessionManager2,
    notification: IAudioSessionNotification,
}

impl Drop for EndpointWatch {
    fn drop(&mut self) {
        let _ = unsafe {
            self.manager
                .UnregisterSessionNotification(&self.notification)
        };
    }
}

struct AudioGuard {
    enumerator: IMMDeviceEnumerator,
    settings: Settings,
    endpoints: HashMap<String, EndpointWatch>,
}

impl AudioGuard {
    fn new(settings: Settings) -> Result<Self> {
        let enumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .context("could not open the Windows audio device enumerator")?;
        Ok(Self {
            enumerator,
            settings,
            endpoints: HashMap::new(),
        })
    }

    fn refresh_endpoints(&mut self) -> Result<()> {
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        }
        .context("could not enumerate active audio output devices")?;
        let count = unsafe { collection.GetCount() }
            .context("could not count active audio output devices")?;
        let mut active_ids = HashSet::new();

        for index in 0..count {
            let device = unsafe { collection.Item(index) }
                .context("could not inspect an audio output device")?;
            let id = device_id(&device)?;
            active_ids.insert(id.clone());

            if !self.endpoints.contains_key(&id) {
                let watch = EndpointWatch::new(&device, &self.settings)
                    .with_context(|| format!("could not watch audio output {id}"))?;
                self.endpoints.insert(id, watch);
            }
        }

        self.endpoints.retain(|id, _| active_ids.contains(id));
        Ok(())
    }
}

impl EndpointWatch {
    fn new(device: &IMMDevice, settings: &Settings) -> Result<Self> {
        let manager: IAudioSessionManager2 = unsafe { device.Activate(CLSCTX_ALL, None) }
            .context("could not activate the audio session manager")?;

        // Microsoft requires the enumerator to be initialized before relying
        // on session-created notifications.
        let sessions = unsafe { manager.GetSessionEnumerator() }
            .context("could not initialize the audio session enumerator")?;

        if settings.include_existing {
            let count = unsafe { sessions.GetCount() }
                .context("could not count existing audio sessions")?;
            for index in 0..count {
                let session = unsafe { sessions.GetSession(index) }
                    .context("could not inspect an existing audio session")?;
                adjust_session(&session, settings)?;
            }
        }

        let notification: IAudioSessionNotification = SessionNotification {
            settings: settings.clone(),
        }
        .into();
        unsafe { manager.RegisterSessionNotification(&notification) }
            .context("could not subscribe to new audio sessions")?;

        Ok(Self {
            manager,
            notification,
        })
    }
}

fn adjust_session(session: &IAudioSessionControl, settings: &Settings) -> Result<()> {
    if !settings.include_system {
        if let Ok(control2) = session.cast::<IAudioSessionControl2>() {
            // S_OK means this is the system-sounds session; S_FALSE means it is not.
            if unsafe { control2.IsSystemSoundsSession() }.0 == 0 {
                return Ok(());
            }
        }
    }

    let volume: ISimpleAudioVolume = session
        .cast()
        .context("audio session has no simple volume control")?;
    let current =
        unsafe { volume.GetMasterVolume() }.context("could not read an audio session's volume")?;
    let target = f32::from(settings.volume) / 100.0;

    if let Some(new_volume) = desired_volume(current, target, settings.cap) {
        unsafe { volume.SetMasterVolume(new_volume, ptr::null()) }
            .context("could not lower an audio session's volume")?;
    }
    Ok(())
}

fn device_id(device: &IMMDevice) -> Result<String> {
    let raw = unsafe { device.GetId() }.context("could not read an audio device ID")?;
    let result = unsafe { raw.to_string() }.map_err(|error| anyhow!(error));
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    result.context("audio device ID was not valid UTF-16")
}

pub(crate) fn run(settings: Settings) -> Result<()> {
    let _com = ComGuard::initialize()?;

    let mutex = unsafe { CreateMutexW(None, false, MUTEX_NAME) }
        .context("could not create the single-instance mutex")?;
    let mutex_already_existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = WinHandle(mutex);
    if mutex_already_existed {
        bail!("windows-volume-guard is already running");
    }

    let stop_event = unsafe { CreateEventW(None, true, false, STOP_EVENT_NAME) }
        .context("could not create the stop event")?;
    let stop_event = WinHandle(stop_event);

    let mut guard = AudioGuard::new(settings)?;
    guard.refresh_endpoints()?;

    loop {
        let wait = unsafe { WaitForSingleObject(stop_event.0, 2_000) };
        if wait == WAIT_OBJECT_0 {
            break;
        }
        // This also picks up USB/Bluetooth outputs attached after startup.
        if let Err(error) = guard.refresh_endpoints() {
            eprintln!("warning: {error:#}");
        }
    }

    drop(mutex);
    Ok(())
}

pub(crate) fn install(settings: Settings) -> Result<()> {
    // Stop an older installed copy before replacing its executable.
    stop_running();
    thread::sleep(Duration::from_millis(2_500));

    let directory = install_directory()?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("could not create {}", directory.display()))?;

    let installed_exe = directory.join(EXE_NAME);
    let current_exe = env::current_exe().context("could not locate the running executable")?;
    if !same_path(&current_exe, &installed_exe) {
        fs::copy(&current_exe, &installed_exe).with_context(|| {
            format!(
                "could not copy {} to {}",
                current_exe.display(),
                installed_exe.display()
            )
        })?;
    }

    let run_arguments = settings_arguments(&settings);
    let launcher = directory.join(LAUNCHER_NAME);
    write_hidden_launcher(&launcher, &installed_exe, &run_arguments)?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey(RUN_KEY)
        .context("could not open the current user's startup registry key")?;
    let startup_command = format!(r#"wscript.exe "{}""#, launcher.display());
    run_key
        .set_value(RUN_VALUE, &startup_command)
        .context("could not enable automatic start")?;

    Command::new("wscript.exe")
        .arg(&launcher)
        .spawn()
        .context("automatic start was installed, but the guard could not be started now")?;

    println!(
        "Installed for this user at {} (new-session volume: {}%).",
        installed_exe.display(),
        settings.volume
    );
    Ok(())
}

pub(crate) fn uninstall() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run_key) = hkcu.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE) {
        match run_key.delete_value(RUN_VALUE) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("could not disable automatic start"),
        }
    }
    stop_running();

    let launcher = install_directory()?.join(LAUNCHER_NAME);
    match fs::remove_file(&launcher) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("could not remove the hidden startup launcher"),
    }

    println!("Automatic start disabled and the running guard was asked to stop.");
    println!(
        "The executable remains at {} and may be deleted manually.",
        install_directory()?.join(EXE_NAME).display()
    );
    Ok(())
}

pub(crate) fn status() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let automatic_start = hkcu
        .open_subkey(RUN_KEY)
        .ok()
        .and_then(|key| key.get_value::<String, _>(RUN_VALUE).ok())
        .is_some();

    let running = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, STOP_EVENT_NAME) }
        .map(|handle| {
            let handle = WinHandle(handle);
            !handle.0.is_invalid()
        })
        .unwrap_or(false);

    println!(
        "Automatic start: {}\nGuard process: {}",
        if automatic_start {
            "enabled"
        } else {
            "disabled"
        },
        if running { "running" } else { "not running" }
    );
    Ok(())
}

fn stop_running() {
    if let Ok(handle) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, STOP_EVENT_NAME) } {
        let handle = WinHandle(handle);
        let _ = unsafe { SetEvent(handle.0) };
    }
}

fn install_directory() -> Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join(APP_DIR))
        .ok_or_else(|| anyhow!("LOCALAPPDATA is not set"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn settings_arguments(settings: &Settings) -> Vec<String> {
    let mut arguments = vec![
        "run".to_owned(),
        "--volume".to_owned(),
        settings.volume.to_string(),
    ];
    if settings.cap {
        arguments.push("--cap".to_owned());
    }
    if settings.include_existing {
        arguments.push("--include-existing".to_owned());
    }
    if settings.include_system {
        arguments.push("--include-system".to_owned());
    }
    arguments
}

fn write_hidden_launcher(path: &Path, executable: &Path, arguments: &[String]) -> Result<()> {
    // Windows file names cannot contain a double quote. Arguments created by
    // settings_arguments are fixed switches and numeric values.
    let command = format!("\"{}\" {}", executable.display(), arguments.join(" "));
    let escaped = command.replace('"', "\"\"");
    let script = format!(
        "Set shell = CreateObject(\"WScript.Shell\")\r\nshell.Run \"{}\", 0, False\r\n",
        escaped
    );
    fs::write(path, script)
        .with_context(|| format!("could not write hidden launcher {}", path.display()))
}
