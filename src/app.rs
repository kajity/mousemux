use crate::cli::{Cli, Command};
use crate::config::{ConfigWarning, LoadResult, has_config_changed, load_config};
use crate::device::{MouseDevice, NormalizedMouseEvent};
use crate::error::AppError;
use crate::router::{HoldBehavior, KeyStroke, RoutedAction, route};
use crate::virtual_keyboard::VirtualKeyboard;
use crate::virtual_mouse::VirtualMouse;
use evdev::KeyCode;
use notify_rust::Notification;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::{Duration, Instant, interval};

pub async fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Some(Command::Check { config }) => run_check(&config),
        Some(Command::Monitor { config }) => run_monitor(&config).await,
        Some(Command::Reload { config }) => run_reload_command(&config),
        None => {
            let config = cli
                .config
                .ok_or_else(|| AppError::Cli("missing --config for daemon mode".to_string()))?;
            run_daemon(&config).await
        }
    }
}

fn run_check(config_path: &Path) -> Result<(), AppError> {
    let load_result = load_config(config_path)?;
    report_warnings(&load_result.warnings);
    log_info(&format!(
        "config OK: {} ({})",
        config_path.display(),
        load_result.config.device_selector.describe()
    ));
    Ok(())
}

async fn run_monitor(config_path: &Path) -> Result<(), AppError> {
    let load_result = load_config(config_path)?;
    report_warnings(&load_result.warnings);

    let mut mouse_device = MouseDevice::open_for_monitor(&load_result.config.device_selector)?;
    log_info(&format!(
        "monitoring config={} selector={} resolved_device={} ({})",
        config_path.display(),
        load_result.config.device_selector.describe(),
        mouse_device.resolved_path().display(),
        mouse_device.resolved_name()
    ));

    let mut sigint = signal(SignalKind::interrupt()).map_err(AppError::Signal)?;
    let mut sigterm = signal(SignalKind::terminate()).map_err(AppError::Signal)?;

    loop {
        tokio::select! {
            _ = sigint.recv() => {
                log_info("received SIGINT, stopping monitor");
                break;
            }
            _ = sigterm.recv() => {
                log_info("received SIGTERM, stopping monitor");
                break;
            }
            event = mouse_device.next_event() => {
                println!("{:?}", event?);
            }
        }
    }

    Ok(())
}

fn run_reload_command(config_path: &Path) -> Result<(), AppError> {
    let pid_path = pid_file_path(config_path);
    let pid = read_pid_file(&pid_path)?;
    send_sighup(pid)?;
    log_info(&format!(
        "requested reload for config={} pid={} via {}",
        config_path.display(),
        pid,
        pid_path.display()
    ));
    Ok(())
}

async fn run_daemon(config_path: &Path) -> Result<(), AppError> {
    let load_result = load_config(config_path)?;
    let mut runtime = Runtime::from_load_result(load_result)?;
    let _pid_file = PidFileGuard::create(config_path)?;

    log_info(&format!(
        "started with config={} selector={} resolved_device={} ({})",
        runtime.config.source_path.display(),
        runtime.config.device_selector.describe(),
        runtime.mouse_device.resolved_path().display(),
        runtime.mouse_device.resolved_name()
    ));

    let mut sigint = signal(SignalKind::interrupt()).map_err(AppError::Signal)?;
    let mut sigterm = signal(SignalKind::terminate()).map_err(AppError::Signal)?;
    let mut sighup = signal(SignalKind::hangup()).map_err(AppError::Signal)?;
    let mut reload_tick = interval(Duration::from_millis(250));
    let mut hold_tick = interval(Duration::from_millis(5));
    let mut last_reload_attempt = Instant::now()
        .checked_sub(Duration::from_millis(runtime.config.reload.debounce_ms))
        .unwrap_or_else(Instant::now);

    loop {
        tokio::select! {
            _ = sigint.recv() => {
                log_info("received SIGINT, shutting down");
                break;
            }
            _ = sigterm.recv() => {
                log_info("received SIGTERM, shutting down");
                break;
            }
            _ = sighup.recv() => {
                runtime.try_reload("signal").await?;
            }
            event = runtime.mouse_device.next_event() => {
                runtime.handle_event(event?)?;
            }
            _ = hold_tick.tick() => {
                runtime.release_due_keys()?;
            }
            _ = reload_tick.tick(), if runtime.config.reload.enabled => {
                if has_config_changed(&runtime.config.source_path, runtime.config.source_modified)?.is_none() {
                    continue;
                }

                let now = Instant::now();
                if now.duration_since(last_reload_attempt).as_millis()
                    < u128::from(runtime.config.reload.debounce_ms)
                {
                    continue;
                }
                last_reload_attempt = now;
                runtime.try_reload("watcher").await?;
            }
        }
    }

    Ok(())
}

struct Runtime {
    config: crate::config::ActiveConfig,
    active_mode_index: usize,
    mouse_device: MouseDevice,
    virtual_mouse: VirtualMouse,
    virtual_keyboard: VirtualKeyboard,
    pending_mouse_events: Vec<NormalizedMouseEvent>,
    pending_keyboard_events: Vec<KeyStroke>,
    active_button_outputs: HashMap<KeyCode, Vec<ActiveButtonOutput>>,
    pressed_output_counts: HashMap<KeyCode, usize>,
    scheduled_releases: Vec<ScheduledRelease>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveButtonOutput {
    key: KeyCode,
    hold: HoldBehavior,
}

#[derive(Clone, Copy, Debug)]
struct ScheduledRelease {
    due_at: Instant,
    key: KeyCode,
}

impl Runtime {
    fn from_load_result(load_result: LoadResult) -> Result<Self, AppError> {
        report_warnings(&load_result.warnings);

        let mouse_device = MouseDevice::open_and_grab(&load_result.config.device_selector)?;
        let virtual_mouse = VirtualMouse::build_from_source_caps(
            mouse_device.source_capabilities(),
            mouse_device.resolved_name(),
        )?;
        let virtual_keyboard = VirtualKeyboard::build(
            load_result.config.rules.registered_keys(),
            mouse_device.resolved_name(),
        )?;

        log_info(&format!(
            "grabbed source device {}",
            mouse_device.resolved_path().display()
        ));

        Ok(Self {
            config: load_result.config,
            active_mode_index: 0,
            mouse_device,
            virtual_mouse,
            virtual_keyboard,
            pending_mouse_events: Vec::new(),
            pending_keyboard_events: Vec::new(),
            active_button_outputs: HashMap::new(),
            pressed_output_counts: HashMap::new(),
            scheduled_releases: Vec::new(),
        })
    }

    async fn try_reload(&mut self, reason: &str) -> Result<(), AppError> {
        match self.apply_reload().await {
            Ok(()) => {
                log_info(&format!(
                    "reloaded ({reason}) config={} selector={} resolved_device={} ({})",
                    self.config.source_path.display(),
                    self.config.device_selector.describe(),
                    self.mouse_device.resolved_path().display(),
                    self.mouse_device.resolved_name()
                ));
                Ok(())
            }
            Err(err) => {
                log_warn(&format!(
                    "reload failed ({reason}) for {}: {err}; keeping previous configuration",
                    self.config.source_path.display()
                ));
                Ok(())
            }
        }
    }

    async fn apply_reload(&mut self) -> Result<(), AppError> {
        let previous_mode_name = self
            .config
            .rules
            .current_mode_name(self.active_mode_index)
            .map(str::to_owned);
        let load_result = load_config(&self.config.source_path)?;
        report_warnings(&load_result.warnings);

        if load_result.config.device_selector != self.config.device_selector {
            let replacement_mouse =
                MouseDevice::open_and_grab(&load_result.config.device_selector)?;
            let replacement_virtual_mouse = VirtualMouse::build_from_source_caps(
                replacement_mouse.source_capabilities(),
                replacement_mouse.resolved_name(),
            )?;
            let replacement_virtual_keyboard = VirtualKeyboard::build(
                load_result.config.rules.registered_keys(),
                replacement_mouse.resolved_name(),
            )?;

            self.reset_keyboard_state()?;
            self.pending_mouse_events.clear();
            self.mouse_device = replacement_mouse;
            self.virtual_mouse = replacement_virtual_mouse;
            self.virtual_keyboard = replacement_virtual_keyboard;
            self.config = load_result.config;
            self.active_mode_index =
                resolve_reloaded_mode_index(&self.config.rules, previous_mode_name);
            return Ok(());
        }

        self.reset_keyboard_state()?;
        self.virtual_keyboard = VirtualKeyboard::build(
            load_result.config.rules.registered_keys(),
            self.mouse_device.resolved_name(),
        )?;
        self.config = load_result.config;
        self.active_mode_index =
            resolve_reloaded_mode_index(&self.config.rules, previous_mode_name);
        Ok(())
    }

    fn handle_event(&mut self, event: NormalizedMouseEvent) -> Result<(), AppError> {
        match route(&event, &self.config.rules, self.active_mode_index) {
            RoutedAction::PassThrough => self.pending_mouse_events.push(event),
            RoutedAction::Remap(sequence) => {
                let sequence = sequence.to_vec();
                self.handle_remap(&event, &sequence);
            }
            RoutedAction::SwitchMode => self.switch_mode(),
            RoutedAction::Flush => self.flush_pending()?,
            RoutedAction::Ignore => {}
        }

        Ok(())
    }

    fn flush_pending(&mut self) -> Result<(), AppError> {
        if !self.pending_mouse_events.is_empty() {
            self.virtual_mouse.emit_frame(&self.pending_mouse_events)?;
            self.pending_mouse_events.clear();
        }

        self.flush_pending_keyboard()?;
        Ok(())
    }

    fn flush_pending_keyboard(&mut self) -> Result<(), AppError> {
        if !self.pending_keyboard_events.is_empty() {
            self.virtual_keyboard
                .emit_frame(&self.pending_keyboard_events)?;
            self.pending_keyboard_events.clear();
        }

        Ok(())
    }

    fn handle_remap(&mut self, event: &NormalizedMouseEvent, sequence: &[KeyStroke]) {
        match event {
            NormalizedMouseEvent::Button { code, value: 1 } => {
                self.handle_button_press(*code, sequence)
            }
            NormalizedMouseEvent::Button { code, value: 0 } => {
                self.handle_button_release(*code, sequence)
            }
            _ => self.pending_keyboard_events.extend_from_slice(sequence),
        }
    }

    fn handle_button_press(&mut self, input_code: KeyCode, sequence: &[KeyStroke]) {
        let mut tracked_outputs = Vec::new();

        for stroke in sequence {
            match stroke.value {
                1 => match stroke.hold {
                    HoldBehavior::Tap => {
                        self.pending_keyboard_events
                            .push(KeyStroke::press(stroke.key));
                        self.pending_keyboard_events
                            .push(KeyStroke::release(stroke.key));
                    }
                    HoldBehavior::FollowInput(_) => {
                        self.press_output_key(stroke.key);
                        tracked_outputs.push(ActiveButtonOutput {
                            key: stroke.key,
                            hold: stroke.hold,
                        });
                    }
                },
                0 => self.release_output_key(stroke.key),
                _ => {}
            }
        }

        if tracked_outputs.is_empty() {
            self.active_button_outputs.remove(&input_code);
        } else {
            self.active_button_outputs
                .insert(input_code, tracked_outputs);
        }
    }

    fn handle_button_release(&mut self, input_code: KeyCode, sequence: &[KeyStroke]) {
        let active_outputs = self
            .active_button_outputs
            .remove(&input_code)
            .unwrap_or_default();
        let tracked_keys = active_outputs
            .iter()
            .map(|output| output.key)
            .collect::<HashSet<_>>();

        for output in active_outputs {
            self.release_output_for_hold(output.key, output.hold);
        }

        for stroke in sequence {
            match stroke.value {
                0 if !tracked_keys.contains(&stroke.key) => {
                    self.release_output_for_hold(stroke.key, stroke.hold)
                }
                1 => self.press_output_key(stroke.key),
                _ => {}
            }
        }
    }

    fn press_output_key(&mut self, key: KeyCode) {
        let count = self.pressed_output_counts.entry(key).or_insert(0);
        if *count == 0 {
            self.pending_keyboard_events.push(KeyStroke::press(key));
        }
        *count += 1;
    }

    fn release_output_key(&mut self, key: KeyCode) {
        let Some(count) = self.pressed_output_counts.get_mut(&key) else {
            return;
        };

        if *count > 1 {
            *count -= 1;
            return;
        }

        self.pressed_output_counts.remove(&key);
        self.pending_keyboard_events.push(KeyStroke::release(key));
    }

    fn release_output_for_hold(&mut self, key: KeyCode, hold: HoldBehavior) {
        match hold {
            HoldBehavior::Tap => {}
            HoldBehavior::FollowInput(0) => self.release_output_key(key),
            HoldBehavior::FollowInput(milliseconds) => {
                self.scheduled_releases.push(ScheduledRelease {
                    due_at: Instant::now() + Duration::from_millis(milliseconds),
                    key,
                });
            }
        }
    }

    fn release_due_keys(&mut self) -> Result<(), AppError> {
        let now = Instant::now();
        let mut retained = Vec::with_capacity(self.scheduled_releases.len());
        let scheduled_releases = std::mem::take(&mut self.scheduled_releases);

        for scheduled in scheduled_releases {
            if scheduled.due_at <= now {
                self.release_output_key(scheduled.key);
            } else {
                retained.push(scheduled);
            }
        }

        self.scheduled_releases = retained;
        self.flush_pending_keyboard()
    }

    fn reset_keyboard_state(&mut self) -> Result<(), AppError> {
        self.active_button_outputs.clear();
        self.scheduled_releases.clear();

        let keys = self
            .pressed_output_counts
            .keys()
            .copied()
            .collect::<Vec<_>>();
        self.pressed_output_counts.clear();

        for key in keys {
            self.pending_keyboard_events.push(KeyStroke::release(key));
        }

        self.flush_pending_keyboard()
    }

    fn switch_mode(&mut self) {
        if self.config.rules.mode_count() <= 1 {
            return;
        }

        let previous_mode = self
            .config
            .rules
            .current_mode_name(self.active_mode_index)
            .unwrap_or("unknown")
            .to_string();
        self.active_mode_index = self.config.rules.next_mode_index(self.active_mode_index);
        let next_mode = self
            .config
            .rules
            .current_mode_name(self.active_mode_index)
            .unwrap_or("unknown");

        log_info(&format!("mode switched: {previous_mode} -> {next_mode}"));
        notify_mode_change(next_mode);
    }
}

struct PidFileGuard {
    path: PathBuf,
    pid: u32,
}

impl PidFileGuard {
    fn create(config_path: &Path) -> Result<Self, AppError> {
        let path = pid_file_path(config_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| AppError::PidFile {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let pid = std::process::id();
        fs::write(&path, pid.to_string()).map_err(|source| AppError::PidFile {
            path: path.clone(),
            source,
        })?;

        log_info(&format!("wrote pid file {}", path.display()));
        Ok(Self { path, pid })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let should_remove = fs::read_to_string(&self.path)
            .ok()
            .and_then(|content| content.trim().parse::<u32>().ok())
            == Some(self.pid);

        if should_remove {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn pid_file_path(config_path: &Path) -> PathBuf {
    let normalized = fs::canonicalize(config_path).unwrap_or_else(|_| config_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    PathBuf::from("/run/mousefold").join(format!("mousefold-{:016x}.pid", hasher.finish()))
}

fn read_pid_file(path: &Path) -> Result<i32, AppError> {
    let content = fs::read_to_string(path).map_err(|source| AppError::PidFile {
        path: path.to_path_buf(),
        source,
    })?;
    content
        .trim()
        .parse::<i32>()
        .map_err(|_| AppError::PidFileFormat {
            path: path.to_path_buf(),
            content,
        })
}

fn send_sighup(pid: i32) -> Result<(), AppError> {
    let result = unsafe { libc::kill(pid, libc::SIGHUP) };
    if result == 0 {
        Ok(())
    } else {
        Err(AppError::SignalSend {
            pid,
            source: std::io::Error::last_os_error(),
        })
    }
}

fn report_warnings(warnings: &[ConfigWarning]) {
    for warning in warnings {
        log_warn(&warning.to_string());
    }
}

fn log_info(message: &str) {
    eprintln!("[INFO] {message}");
}

fn log_warn(message: &str) {
    eprintln!("[WARN] {message}");
}

fn resolve_reloaded_mode_index(
    rules: &crate::router::CompiledRules,
    previous_mode_name: Option<String>,
) -> usize {
    previous_mode_name
        .as_deref()
        .and_then(|mode_name| rules.find_mode_index(mode_name))
        .unwrap_or(0)
}

fn notify_mode_change(mode_name: &str) {
    if let Err(err) = Notification::new()
        .summary("mousefold")
        .body(&format!("Mode changed to {mode_name}"))
        .show()
    {
        log_warn(&format!(
            "failed to send desktop notification for mode switch: {err}"
        ));
    }
}
