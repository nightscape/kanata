use kanata_parser::cfg::*;
use std::sync::{Mutex, MutexGuard};

#[cfg(all(
    feature = "simulated_output",
    not(feature = "simulated_input"),
    not(feature = "interception_driver")
))]
mod sim_tests;

#[cfg(all(
    target_os = "macos",
    feature = "simulated_input",
    feature = "simulated_output"
))]
mod passthru_macos_tests;

static CFG_PARSE_LOCK: Mutex<()> = Mutex::new(());

/// Guards the process-global state that building a `Kanata` installs: `MAPPED_KEYS`,
/// `PRESSED_KEYS` and the zippychord dictionary (`ZCH`). Every successful
/// `Kanata::new_from_str` overwrites all of it — a config without `defzippy` wipes the
/// zippychord dictionary to empty — so a test must hold this for as long as it uses the
/// instance, not merely across construction.
///
/// A failing test panics while holding it, so poison is routine and carries no meaning.
pub(crate) fn cfg_parse_guard() -> MutexGuard<'static, ()> {
    CFG_PARSE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn init_log() {
    use simplelog::*;
    use std::sync::OnceLock;
    static LOG_INIT: OnceLock<()> = OnceLock::new();
    LOG_INIT.get_or_init(|| {
        let mut log_cfg = ConfigBuilder::new();
        if let Err(e) = log_cfg.set_time_offset_to_local() {
            eprintln!("WARNING: could not set log TZ to local: {e:?}");
        };
        log_cfg.set_time_format_rfc3339();
        CombinedLogger::init(vec![TermLogger::new(
            // Note: set to a different level to see logs in tests.
            // Also, not all tests call init_log so you might have to add the call there too.
            LevelFilter::Off,
            log_cfg.build(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        )])
        .expect("logger can init");
    });
}

// Do not run during simulated input testing
// because those will manipulate the deflocalkeys globals
// which interferes with these tests.
#[cfg(not(feature = "simulated_input"))]
mod parse_samples {
    use super::*;

    #[test]
    fn parse_simple() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/simple.kbd")).unwrap();
    }

    #[test]
    fn parse_gamepad() {
        init_log();
        let _lk = match CFG_PARSE_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let cfg = new_from_file(&std::path::PathBuf::from("./cfg_samples/gamepad.kbd")).unwrap();
        // The sample is the documentation's worked example, so it should
        // exercise the declaration rather than merely parse.
        let gamepad = cfg.gamepad.expect("the sample declares a controller");
        assert!(gamepad.drives_motion());
    }

    #[test]
    fn parse_minimal() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/minimal.kbd")).unwrap();
    }

    #[test]
    fn parse_deflayermap() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/deflayermap.kbd")).unwrap();
    }

    #[test]
    fn parse_default() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/kanata.kbd")).unwrap();
    }

    #[test]
    fn parse_jtroo() {
        init_log();
        let _lk = cfg_parse_guard();
        let cfg = new_from_file(&std::path::PathBuf::from("./cfg_samples/jtroo.kbd")).unwrap();
        assert_eq!(cfg.layer_info.len(), 8);
    }

    #[test]
    fn parse_f13_f24() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/f13_f24.kbd")).unwrap();
    }

    #[test]
    fn parse_home_row_mods() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from(
            "./cfg_samples/home-row-mod-basic.kbd",
        ))
        .unwrap();
        new_from_file(&std::path::PathBuf::from(
            "./cfg_samples/home-row-mod-advanced.kbd",
        ))
        .unwrap();
    }

    #[test]
    fn parse_press_release_toggle_vkeys() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from(
            "./cfg_samples/key-toggle_press-only_release-only.kbd",
        ))
        .unwrap();
    }

    #[test]
    fn parse_automousekeys_only() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from(
            "./cfg_samples/automousekeys-only.kbd",
        ))
        .unwrap();
    }

    #[test]
    fn parse_automousekeys_full_map() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from(
            "./cfg_samples/automousekeys-full-map.kbd",
        ))
        .unwrap();
    }

    #[test]
    fn parse_push_msg() {
        init_log();
        let _lk = cfg_parse_guard();
        new_from_file(&std::path::PathBuf::from("./cfg_samples/push-msg.kbd")).unwrap();
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn sizeof_state() {
        init_log();
        assert_eq!(
            std::mem::size_of::<
                kanata_keyberon::layout::State<
                    &'static &'static [&'static kanata_parser::custom_action::CustomAction],
                >,
            >(),
            2 * std::mem::size_of::<usize>()
        );
    }
}
