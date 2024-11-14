use super::sexpr::SExpr;
use std::collections::HashSet;
use super::error::*;
use kanata_keyberon::key_code::KeyCode;

#[cfg(any(target_os = "linux", target_os = "unknown"))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeviceDetectMode {
    KeyboardOnly,
    KeyboardMice,
    Any,
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
impl std::fmt::Display for DeviceDetectMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
#[derive(Debug, Clone)]
pub struct CfgLinuxOptions {
    pub linux_dev: Vec<String>,
    pub linux_dev_names_include: Option<Vec<String>>,
    pub linux_dev_names_exclude: Option<Vec<String>>,
    pub linux_continue_if_no_devs_found: bool,
    pub linux_unicode_u_code: KeyCode,
    pub linux_unicode_termination: UnicodeTermination,
    pub linux_x11_repeat_delay_rate: Option<KeyRepeatSettings>,
    pub linux_use_trackpoint_property: bool,
    pub linux_output_bus_type: LinuxCfgOutputBusType,
    pub linux_device_detect_mode: Option<DeviceDetectMode>,
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
impl Default for CfgLinuxOptions {
    fn default() -> Self {
        Self {
            linux_dev: vec![],
            linux_dev_names_include: None,
            linux_dev_names_exclude: None,
            linux_continue_if_no_devs_found: false,
            linux_unicode_u_code: KeyCode::U,
            linux_unicode_termination: UnicodeTermination::Enter,
            linux_x11_repeat_delay_rate: None,
            linux_use_trackpoint_property: false,
            linux_output_bus_type: LinuxCfgOutputBusType::BusI8042,
            linux_device_detect_mode: None,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
#[derive(Debug, Clone, Copy)]
pub enum LinuxCfgOutputBusType {
    BusUsb,
    BusI8042,
    BusSerio,
}

#[cfg(all(any(target_os = "windows", target_os = "unknown"), feature = "gui"))]
#[derive(Debug, Clone)]
pub struct CfgOptionsGui {
    pub window_x: u16,
    pub window_y: u16,
    pub window_width_px: u16,
    pub window_height_px: u16,
    pub font_size: u16,
    pub font_family: String,
    pub log_retention_ms: u16,
    pub dark_theme: bool,
    pub transparency: u8,
    pub always_on_top: bool,
    pub show_logs_on_error: bool,
    pub show_on_startup: bool,
}

#[cfg(all(any(target_os = "windows", target_os = "unknown"), feature = "gui"))]
impl Default for CfgOptionsGui {
    fn default() -> Self {
        Self {
            window_x: 100,
            window_y: 100,
            window_width_px: 800,
            window_height_px: 600,
            font_size: 11,
            font_family: "Consolas".to_string(),
            log_retention_ms: 5000,
            dark_theme: true,
            transparency: 255,
            always_on_top: false,
            show_logs_on_error: true,
            show_on_startup: true,
        }
    }
}

#[derive(Debug)]
pub struct CfgOptions {
    pub process_idle_timeout_ms: u16,
    pub delegate_to_first_layer: bool,
    pub sequence_timeout_ms: u16,
    pub sequence_input_mode: bool,
    pub sequence_backtrack_modcancel: bool,
    pub tap_hold_wait_time_ms: u16,
    pub tap_hold_wait_time_ms_fast: Option<u16>,
    pub tap_hold_wait_time_ms_slower: Option<u16>,
    pub tap_hold_wait_time_ms_slowest: Option<u16>,
    pub tap_hold_wait_time_ms_leader: Option<u16>,
    pub tap_hold_wait_time_ms_tapdance: Option<u16>,
    pub dynamic_macro_wait_time_ms: u16,
    pub chord_hold_wait_time_ms: u16,
    pub chord_timeout_ms: u16,
    pub quick_tap_ms: Option<u16>,
    pub quick_tap_ms_fast: Option<u16>,
    pub quick_tap_ms_slower: Option<u16>,
    pub quick_tap_ms_slowest: Option<u16>,
    pub quick_tap_ms_leader: Option<u16>,
    pub quick_tap_ms_tapdance: Option<u16>,
    pub layer_lock_ms: Option<u16>,
    pub default_log_level: log::Level,
    pub log_file: Option<String>,
    pub log_file_reopen_delay_ms: Option<u16>,
    pub log_error_file: Option<String>,
    pub log_error_file_reopen_delay_ms: Option<u16>,
    pub danger_enable_cmd: bool,
    pub danger_enable_cmd_output_keys: bool,
    pub danger_enable_arbitrary_code: bool,
    pub danger_enable_tcp_server: bool,
    pub danger_enable_tcp_server_port: Option<u16>,
    pub danger_enable_tcp_server_read_only: bool,
    pub danger_enable_live_reload: bool,
    pub danger_enable_file_watch: bool,
    pub danger_enable_file_watch_paths: Vec<String>,
    pub danger_enable_file_watch_inputs: Vec<String>,
    pub danger_enable_file_watch_outputs: Vec<String>,
    pub danger_enable_file_watch_delay_ms: Option<u16>,
    pub danger_enable_file_watch_log_level: log::Level,
    pub danger_enable_file_watch_log_file: Option<String>,
    pub danger_enable_file_watch_log_error_file: Option<String>,
    pub danger_enable_file_watch_log_file_reopen_delay_ms: Option<u16>,
    pub danger_enable_file_watch_log_error_file_reopen_delay_ms: Option<u16>,
}

impl Default for CfgOptions {
    fn default() -> Self {
        Self {
            process_idle_timeout_ms: 500,
            delegate_to_first_layer: false,
            sequence_timeout_ms: 1000,
            sequence_input_mode: false,
            sequence_backtrack_modcancel: false,
            tap_hold_wait_time_ms: 200,
            tap_hold_wait_time_ms_fast: None,
            tap_hold_wait_time_ms_slower: None,
            tap_hold_wait_time_ms_slowest: None,
            tap_hold_wait_time_ms_leader: None,
            tap_hold_wait_time_ms_tapdance: None,
            dynamic_macro_wait_time_ms: 50,
            chord_hold_wait_time_ms: 20,
            chord_timeout_ms: 50,
            quick_tap_ms: None,
            quick_tap_ms_fast: None,
            quick_tap_ms_slower: None,
            quick_tap_ms_slowest: None,
            quick_tap_ms_leader: None,
            quick_tap_ms_tapdance: None,
            layer_lock_ms: None,
            default_log_level: log::Level::Info,
            log_file: None,
            log_file_reopen_delay_ms: None,
            log_error_file: None,
            log_error_file_reopen_delay_ms: None,
            danger_enable_cmd: false,
            danger_enable_cmd_output_keys: false,
            danger_enable_arbitrary_code: false,
            danger_enable_tcp_server: false,
            danger_enable_tcp_server_port: None,
            danger_enable_tcp_server_read_only: false,
            danger_enable_live_reload: false,
            danger_enable_file_watch: false,
            danger_enable_file_watch_paths: vec![],
            danger_enable_file_watch_inputs: vec![],
            danger_enable_file_watch_outputs: vec![],
            danger_enable_file_watch_delay_ms: None,
            danger_enable_file_watch_log_level: log::Level::Info,
            danger_enable_file_watch_log_file: None,
            danger_enable_file_watch_log_error_file: None,
            danger_enable_file_watch_log_file_reopen_delay_ms: None,
            danger_enable_file_watch_log_error_file_reopen_delay_ms: None,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct KeyRepeatSettings {
    pub delay: u16,
    pub rate: u16,
}

#[cfg(any(target_os = "linux", target_os = "unknown"))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnicodeTermination {
    Enter,
    Space,
    None,
}
