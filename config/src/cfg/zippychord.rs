//! Zipchord-like parsing. Probably not 100% compatible.
//!
//! Example lines in input file.
//! The " => " string represents a tab character.
//!
//! "dy => day"
//!   -> chord: (d y)
//!   -> output: "day"
//!
//! "dy => day"
//! "dy 1 => Monday"
//!   -> chord: (d y)
//!   -> output: "day"
//!   -> chord: (d y)
//!   -> output: "Monday"; "day" gets erased
//!
//! " abc => Alphabet"
//!   -> chord: (space a b c)
//!   -> output: "Alphabet"
//!
//! "r df => recipient"
//!   -> chord: (r)
//!   -> output: nothing yet, just type r
//!   -> chord: (d f)
//!   -> output: "recipient"
//!
//! " w  a => Washington"
//!   -> chord: (space w)
//!   -> output: nothing yet, type spacebar+w in whatever true order they were pressed
//!   -> chord: (space a)
//!   -> output: "Washington"
//!   -> note: do observe the two spaces between 'w' and 'a'
use super::*;

use crate::subset::*;

use parking_lot::Mutex;

/// All possible chords.
#[derive(Debug, Clone, Default)]
pub struct ZchPossibleChords(pub SubsetMap<u16, Arc<ZchChordOutput>>);
impl ZchPossibleChords {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Tracks current input to check against possible chords.
/// This does not store by the input order;
/// instead it is by some consistent ordering for
/// hashing into the possible chord map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZchInputKeys {
    zch_inputs: ZchSortedChord,
}
impl ZchInputKeys {
    pub fn zchik_new() -> Self {
        Self {
            zch_inputs: ZchSortedChord {
                zch_keys: Vec::new(),
            },
        }
    }
    pub fn zchik_contains(&self, osc: OsCode) -> bool {
        self.zch_inputs.zch_keys.contains(&osc.into())
    }
    pub fn zchik_insert(&mut self, osc: OsCode) {
        self.zch_inputs.zch_insert(osc.into());
    }
    pub fn zchik_remove(&mut self, osc: OsCode) {
        self.zch_inputs.zch_keys.retain(|k| *k != osc.into());
    }
    pub fn zchik_len(&self) -> usize {
        self.zch_inputs.zch_keys.len()
    }
    pub fn zchik_clear(&mut self) {
        self.zch_inputs.zch_keys.clear()
    }
    pub fn zchik_keys(&self) -> &[u16] {
        &self.zch_inputs.zch_keys
    }
    pub fn zchik_is_empty(&self) -> bool {
        self.zch_inputs.zch_keys.is_empty()
    }
}

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
/// Sorted consistently by some arbitrary key order;
/// as opposed to, for example, simply the user press order.
pub struct ZchSortedChord {
    zch_keys: Vec<u16>,
}
impl ZchSortedChord {
    pub fn zch_insert(&mut self, key: u16) {
        match self.zch_keys.binary_search(&key) {
            // Q: what is the meaning of Ok vs. Err?
            // A: Ok means the element already in vector @ `pos`. Normally this wouldn't be
            // expected to happen but it turns out that key repeat might get in the way of this
            // assumption. Err means element does not exist and returns the correct insert position.
            Ok(_pos) => {}
            Err(pos) => self.zch_keys.insert(pos, key),
        }
    }
}

/// A chord.
///
/// If any followups exist it will be Some.
/// E.g. with:
/// - dy   -> day
/// - dy 1 -> Monday
/// - dy 2 -> Tuesday
///
/// the output will be "day" and the Monday+Tuesday chords will be in `followups`.
#[derive(Debug, Clone)]
pub struct ZchChordOutput {
    pub zch_output: Box<[ZchOutput]>,
    pub zch_followups: Option<Arc<Mutex<ZchPossibleChords>>>,
}

/// Zch output can be uppercase, lowercase, altgr, and shift-altgr characters.
/// The parser should ensure all `OsCode`s in variants containing them
/// are visible characters that are backspacable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZchOutput {
    Lowercase(OsCode),
    Uppercase(OsCode),
    AltGr(OsCode),
    ShiftAltGr(OsCode),
}

impl ZchOutput {
    pub fn osc(self) -> OsCode {
        use ZchOutput::*;
        match self {
            Lowercase(osc) | Uppercase(osc) | AltGr(osc) | ShiftAltGr(osc) => osc,
        }
    }
    pub fn display_len(outs: impl AsRef<[Self]>) -> i16 {
        outs.as_ref().iter().copied().fold(0i16, |mut len, out| {
            len += match out.osc() {
                OsCode::KEY_BACKSPACE => -1,
                _ => 1,
            };
            len
        })
    }
}

/// User configuration for smart space.
///
/// - `Full`         = add spaces after words, remove these spaces after typing punctuation.
/// - `AddSpaceOnly` = add spaces after words
/// - `Disabled`     = do nothing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZchSmartSpaceCfg {
    Full,
    AddSpaceOnly,
    Disabled,
}

#[derive(Debug)]
pub struct ZchConfig {
    /// When, during typing, chord fails to activate, zippychord functionality becomes temporarily
    /// disabled. This is to avoid accidental chord activations when typing normally, as opposed to
    /// intentionally trying to activate a chord. The duration of temporary disabling is determined
    /// by this configuration item. Re-enabling also happens when word-splitting characters are
    /// typed, for example typing  a space or a comma, but a pause of all typing activity lasting a
    /// number of milliseconds equal to this configuration will also re-enable chording even if
    /// typing within a single word.
    pub zch_cfg_ticks_wait_enable: u16,

    /// Assuming zippychording is enabled, when the first press happens this deadline will begin
    /// and if no chords are completed within the deadline, zippychording will be disabled
    /// temporarily (see `zch_cfg_ticks_wait_enable`). You may want a long or short deadline
    /// depending on your use case. If you are primarily typing normally, with chords being used
    /// occasionally being used, you may want a short deadline so that regular typing will be
    /// unlikely to activate any chord. However, if you primarily type with chords, you may want a
    /// longer deadline to give you more time to complete the intended chord (e.g. in case of
    /// overlaps). With a long deadline you should be very intentional about pressing and releasing
    /// an individual key to begin a sequence of regular typing to trigger the disabling of
    /// zippychord. If, after the first press, a chord activates, this deadline will reset to
    /// enable further chord activations.
    pub zch_cfg_ticks_chord_deadline: u16,

    /// User configuration for smart space. See `pub enum ZchSmartSpaceCfg`.
    pub zch_cfg_smart_space: ZchSmartSpaceCfg,

    /// Define keys for punctuation, which is relevant to smart space auto-erasure of added spaces.
    pub zch_cfg_smart_space_punctuation: HashSet<ZchOutput>,
}

impl Default for ZchConfig {
    fn default() -> Self {
        Self {
            zch_cfg_ticks_wait_enable: 500,
            zch_cfg_ticks_chord_deadline: 500,
            zch_cfg_smart_space: ZchSmartSpaceCfg::Disabled,
            zch_cfg_smart_space_punctuation: {
                let mut puncs = HashSet::default();
                puncs.insert(ZchOutput::Lowercase(OsCode::KEY_DOT));
                puncs.insert(ZchOutput::Lowercase(OsCode::KEY_COMMA));
                puncs.insert(ZchOutput::Lowercase(OsCode::KEY_SEMICOLON));
                puncs.shrink_to_fit();
                puncs
            },
        }
    }
}

