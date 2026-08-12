use super::*;

use kanata_parser::subset::GetOrIsSubsetOfKnownKey::*;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

// Maybe-todos:
// ---
// Feature-parity: suffixes - only active while disabled, to complete a word.
// Feature-parity: prefix vs. non-prefix. Assuming smart spacing is implemented and enabled,
//                 standard activations would output space one outputs space, but not prefixes.
//                 I guess can be done in parser.

static ZCH: Lazy<Mutex<ZchState>> = Lazy::new(|| Mutex::new(Default::default()));

pub(crate) fn zch() -> MutexGuard<'static, ZchState> {
    match ZCH.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let mut inner = poisoned.into_inner();
            inner.zchd.zchd_reset();
            inner
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
enum ZchEnabledState {
    #[default]
    Enabled,
    WaitEnable,
    Disabled,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ZchLastPressClassification {
    #[default]
    IsChord,
    NotChord,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ZchSmartSpaceState {
    #[default]
    Inactive,
    Sent,
}

/// Whether an activation decides its own trailing smart space or replays one already
/// seen on screen.
#[derive(Debug, Clone, Copy)]
enum ZchTrailingSpace {
    Decide,
    Replay(bool),
}

/// What a provisional followup activation must know to be undone in favour of the
/// orphan-free parse. The record captures everything needed to reproduce the replaced
/// activation's OBSERVED output — its identity and every context-dependent rendering
/// decision — because the restore replays that output rather than deciding afresh:
/// what reappears must be exactly the text the followup replaced, or the delete budgets
/// and `zchd_on_screen` inherit the mismatch.
#[derive(Debug)]
struct ZchFollowupUndo {
    /// The chord the followup replaced; re-activating it restores both its word and
    /// its followup map.
    replaced: Arc<ZchChordOutput>,
    /// The keys the followup consumed, in `ZchInputKeys`' own encoding. They are
    /// cleared from the input set at activation (so a chain link accumulates from
    /// empty), so they are kept here to complete a main chord alongside the keys
    /// pressed after them.
    keys: Vec<u16>,
    /// The key that opened this gesture, before the followup's activation emptied the
    /// input set and re-armed it. `not-first-def-key` compares against the first key
    /// the user pressed, and correcting the parse does not change which that was.
    first_pressed: Option<OsCode>,
    /// Whether the replaced activation put a trailing smart space on screen.
    added_trailing_space: bool,
}

#[derive(Debug, Default)]
struct ZchDynamicState {
    /// Input to compare against configured available chords to output.
    zchd_input_keys: ZchInputKeys,
    /// Whether chording should be enabled or disabled.
    /// Chording will be disabled if:
    /// - further presses cannot possibly activate a chord
    /// - a release happens with no chord having been activated
    ///
    /// Once disabled, chording will be enabled when:
    /// - all keys have been released
    /// - zchd_ticks_until_enabled shrinks to 0
    zchd_enabled_state: ZchEnabledState,
    /// A complete chord match whose activation has been *deferred* because the keys
    /// currently held are also a proper subset of a longer chord that could still be
    /// completed (e.g. `er`->"error" while the keys of `sure` are arriving). Its
    /// keys are echoed through meanwhile; it fires only if nothing longer completes —
    /// when the chord deadline expires (`zch_tick`) or the keys start being released
    /// (`zch_release_key`). A later press that completes a longer chord supersedes it.
    /// This is what prevents over-eager expansion (the visible error->response->sure
    /// churn). See [[zippy-pbt-layout-blindspot]] / ZIPPY_PBT_NOTES.md.
    zchd_deferred: Option<Arc<ZchChordOutput>>,
    /// Is Some when a chord has been activated which has possible follow-up chords.
    /// E.g. dy -> day
    ///      dy 1 -> Monday
    ///      dy 2 -> Tuesday
    /// Using the example above, when dy has been activated, the `1` and `2` activations will be
    /// contained within `zchd_prioritized_chords`. This is cleared if the input is such that an
    /// activation is no longer possible.
    zchd_prioritized_chords: Option<Arc<parking_lot::Mutex<ZchPossibleChords>>>,
    /// Tracks the prior output character count
    /// because it may need to be erased (see `zchd_prioritized_chords).
    zchd_prior_activation_output_count: i16,
    /// Tracks the number of characters typed to complete an activation, which will be erased if an
    /// activation completes succesfully.
    zchd_characters_to_delete_on_next_activation: i16,
    /// Everything zippychord currently has on screen in its own owned region, in
    /// order: eagerly echoed input keys, the most recent activation's output, and
    /// any trailing smart space — exactly what is visible before the next
    /// activation runs. The next activation's common prefix is measured against
    /// this, so any character already correct (an echoed input key the output
    /// extends, a prior activation's output an overlapping chord shares, or a
    /// trailing smart space the new word's space lands on) is preserved instead of
    /// backspaced and re-typed. Persists across a key release while a followup is
    /// pending (the followup reconciles against it); cleared on any reset.
    zchd_on_screen: Vec<ZchOutput>,
    /// How many trailing `zchd_on_screen` entries are eagerly echoed input keys that no
    /// activation has replaced yet. Cancelling a pending followup keeps exactly these —
    /// their keys are still held and their characters are still on screen — and drops the
    /// finished word before them.
    zchd_echo_len: i16,
    /// Tracker for time until prior state change to know if potential stale data should be
    /// cleared. This is a contingency in case of bugs or weirdness with OS interactions, e.g.
    /// Windows lock screen weirdness.
    ///
    /// This counts upwards to a "reset state" number.
    zchd_ticks_since_state_change: u16,
    /// Zch has a time delay between being disabled->pending-enabled->truly-enabled to mitigate
    /// against unintended activations. This counts downwards from a configured number until 0, and
    /// at 0 the state transitions from pending-enabled to truly-enabled if applicable.
    zchd_ticks_until_enabled: u16,
    /// There is a deadline between the first press happening and a chord activation being
    /// possible; after which if a chord has not been activated, zippychording is disabled. This
    /// state is the counter for this deadline.
    zchd_ticks_until_disable: u16,
    /// Current state of caps-word, which is a factor in handling capitalization.
    zchd_is_caps_word_active: bool,
    /// Current state of lsft which is a factor in handling capitalization.
    zchd_is_lsft_active: bool,
    /// Current state of rsft which is a factor in handling capitalization.
    zchd_is_rsft_active: bool,
    /// Current state of altgr which is a factor in smart space erasure.
    zchd_is_altgr_active: bool,
    /// Tracks whether last press was part of a chord or not.
    /// Upon releasing keys, this state determines if zippychording should remain enabled or
    /// disabled.
    zchd_last_press: ZchLastPressClassification,
    /// Tracks smart spacing state so punctuation characters
    /// can know whether a space needs to be erased or not.
    zchd_smart_space_state: ZchSmartSpaceState,
    /// First key pressed in the current input sequence (re-armed whenever input
    /// goes from empty to non-empty). Used by the `not-first-def-key`
    /// suppress-space mode to compare against the activated chord's first
    /// definition key.
    zchd_first_pressed_key: Option<OsCode>,
    /// The chord whose output is currently the last thing this module wrote. A
    /// followup activation records the one it replaced, so the word can be put back.
    zchd_last_activation: Option<Arc<ZchChordOutput>>,
    /// Whether `zchd_last_activation` ended in a trailing smart space.
    zchd_last_activation_added_space: bool,
    /// Live while a followup's own keys are still held. Martin's rule prefers the
    /// parse in which every pressed key participates, so as long as those keys can
    /// still join a main chord the followup's word is provisional: completing that
    /// chord puts the replaced word back and types the main chord's word instead.
    /// Dies at the first release of a recorded key, which is when the alternative
    /// parse stops being reachable.
    zchd_followup_undo: Option<ZchFollowupUndo>,
    /// Held state of the configured suppress-space key (mode `(key <key>)`),
    /// tracked like altgr: toggled only by that key's own press/release.
    zchd_is_suppress_space_active: bool,
}

impl ZchDynamicState {
    /// Returns true when the chord deadline just elapsed with a `deferred` chord
    /// pending: the caller (`zch_tick`, which has keyboard-output access) must fire it.
    fn zchd_tick(&mut self, is_caps_word_active: bool, layout_pending: bool) -> bool {
        const TICKS_UNTIL_FORCE_STATE_RESET: u16 = 10000;
        let mut fire_deferred = false;
        self.zchd_ticks_since_state_change += 1;
        self.zchd_is_caps_word_active = is_caps_word_active;
        match self.zchd_enabled_state {
            ZchEnabledState::WaitEnable => {
                self.zchd_ticks_until_enabled = self.zchd_ticks_until_enabled.saturating_sub(1);
                if self.zchd_ticks_until_enabled == 0 {
                    log::debug!("zippy wait enable->enable");
                    self.zchd_enabled_state = ZchEnabledState::Enabled;
                    self.zchd_ticks_until_disable = 0;
                }
            }
            ZchEnabledState::Enabled => {
                // Only run disable-check logic if ticks is already greater than zero, because zero
                // means deadline has never been triggered by any press.
                //
                // Freeze the deadline while the layout is still deferring output (a
                // tap-hold / chord decision is pending or events are queued behind
                // one). The deadline measures how fast the *user* pressed the chord
                // keys; a participating key routed through a tap-hold has its output
                // delayed by the layout, not by the user. Without this freeze the
                // outcome is press-order dependent: when the tap-hold key is pressed
                // first it queues the other key so both arrive together, but when the
                // plain key is pressed first it races ahead and the deadline can
                // expire before the delayed key ever arrives (chord lost).
                if !layout_pending && self.zchd_ticks_until_disable > 0 {
                    self.zchd_ticks_until_disable = self.zchd_ticks_until_disable.saturating_sub(1);
                    if self.zchd_ticks_until_disable == 0 {
                        if self.zchd_deferred.is_some() {
                            // Deadline elapsed with a deferred complete chord pending and
                            // nothing longer completed: fire the deferred chord (the user
                            // settled on it). Done by `zch_tick`, which has keyboard output.
                            // Takes precedence over cancelling a followup left pending by an
                            // earlier chord: the activation needs `zchd_on_screen` and the
                            // delete counter that `zchd_clear_history` drops, and it installs
                            // its own followups, which cancels the pending one.
                            log::debug!("zippy deadline elapsed->fire deferred chord");
                            fire_deferred = true;
                        } else if self.zchd_prioritized_chords.is_some() {
                            // Followup deadline elapsed: cancel the pending followup but stay
                            // enabled and ready for a fresh chord (a deliberate activation already
                            // happened — this is the same readiness as after any activation, not the
                            // "disable to avoid accidental chords during normal typing" case).
                            log::debug!("zippy followup deadline elapsed->clear followup");
                            self.zchd_cancel_followup();
                        } else {
                            // Initial deadline elapsed with no chord: disable to avoid accidental
                            // activations during ordinary typing.
                            log::debug!("zippy enable->disable");
                            self.zchd_soft_reset();
                        }
                    }
                }
            }
            ZchEnabledState::Disabled => {}
        }
        if self.zchd_ticks_since_state_change > TICKS_UNTIL_FORCE_STATE_RESET {
            self.zchd_reset();
        }
        fire_deferred
    }

    fn zchd_state_change(&mut self, cfg: &ZchConfig) {
        self.zchd_ticks_since_state_change = 0;
        self.zchd_ticks_until_enabled = cfg.zch_cfg_ticks_wait_enable;
    }

    fn zchd_activate_chord_deadline(&mut self, deadline_ticks: u16) {
        if self.zchd_ticks_until_disable == 0 {
            self.zchd_ticks_until_disable = deadline_ticks;
        }
    }

    fn zchd_restart_deadline(&mut self, deadline_ticks: u16) {
        self.zchd_ticks_until_disable = deadline_ticks;
    }

    /// Clean up the state, potentially causing inaccuracies with regards to what the user is
    /// currently still pressing.
    fn zchd_reset(&mut self) {
        log::debug!("zchd reset state");
        self.zchd_soft_reset();
        self.zchd_is_caps_word_active = false;
        self.zchd_is_lsft_active = false;
        self.zchd_is_rsft_active = false;
        self.zchd_is_altgr_active = false;
        self.zchd_last_press = ZchLastPressClassification::IsChord;
        self.zchd_enabled_state = ZchEnabledState::Enabled;
    }

    fn zchd_soft_reset(&mut self) {
        log::debug!("zchd soft reset state");
        self.zchd_last_press = ZchLastPressClassification::NotChord;
        self.zchd_enabled_state = ZchEnabledState::Disabled;
        self.zchd_input_keys.zchik_clear();
        self.zchd_ticks_since_state_change = 0;
        self.zchd_ticks_until_disable = 0;
        self.zchd_ticks_until_enabled = 0;
        self.zchd_smart_space_state = ZchSmartSpaceState::Inactive;
        self.zchd_first_pressed_key = None;
        self.zchd_clear_history();
    }

    fn zchd_clear_history(&mut self) {
        log::debug!("zchd clear historical data");
        self.zchd_characters_to_delete_on_next_activation = 0;
        self.zchd_prioritized_chords = None;
        self.zchd_deferred = None;
        self.zchd_prior_activation_output_count = 0;
        self.zchd_on_screen.clear();
        self.zchd_echo_len = 0;
        self.zchd_last_activation = None;
        self.zchd_followup_undo = None;
    }

    /// Cancel a pending followup once its deadline elapses. The finished word stops being
    /// ours to rewrite, while keys echoed since then are still held and still on screen,
    /// so their bookkeeping carries over to the activation still being formed.
    fn zchd_cancel_followup(&mut self) {
        log::debug!("zchd cancel pending followup");
        self.zchd_prioritized_chords = None;
        self.zchd_deferred = None;
        self.zchd_followup_undo = None;
        self.zchd_prior_activation_output_count = 0;
        assert!(
            self.zchd_echo_len as usize <= self.zchd_on_screen.len(),
            "echoed {} keys but only {} entries on screen",
            self.zchd_echo_len,
            self.zchd_on_screen.len()
        );
        let finished_word_len = self.zchd_on_screen.len() - self.zchd_echo_len as usize;
        self.zchd_on_screen.drain(..finished_word_len);
        self.zchd_characters_to_delete_on_next_activation = self.zchd_echo_len;
    }

    /// Returns true if dynamic zch state is such that idling optimization can activate.
    fn zchd_is_idle(&self) -> bool {
        // A live disable deadline (`ticks_until_disable > 0`) must keep ticking so it can expire —
        // e.g. a pending followup that should be cancelled once the followup deadline elapses. If
        // this reported idle, the idle optimization would freeze the countdown and the followup
        // would persist indefinitely across a pause.
        let is_idle = self.zchd_enabled_state == ZchEnabledState::Enabled
            && self.zchd_input_keys.zchik_is_empty()
            && self.zchd_ticks_until_disable == 0;
        log::trace!("zch is idle: {is_idle}");
        is_idle
    }

    fn zchd_press_key(&mut self, osc: OsCode) {
        if self.zchd_input_keys.zchik_is_empty() {
            self.zchd_first_pressed_key = Some(osc);
        }
        self.zchd_input_keys.zchik_insert(osc);
    }

    fn zchd_release_key(&mut self, osc: OsCode, followup_deadline_ticks: u16) {
        self.zchd_input_keys.zchik_remove(osc);
        match (self.zchd_last_press, self.zchd_input_keys.zchik_is_empty()) {
            (ZchLastPressClassification::NotChord, true) => {
                log::debug!("all released->zippy wait enable");
                self.zchd_enabled_state = ZchEnabledState::WaitEnable;
                self.zchd_clear_history();
            }
            (ZchLastPressClassification::NotChord, false) => {
                log::debug!("release but not all->zippy disable");
                self.zchd_soft_reset();
            }
            (ZchLastPressClassification::IsChord, true) => {
                log::debug!("all released->zippy enabled");
                self.zchd_characters_to_delete_on_next_activation = 0;
                self.zchd_enabled_state = ZchEnabledState::Enabled;
                if self.zchd_prioritized_chords.is_none() {
                    log::debug!("no continuation->zippy clear key erase state");
                    // No followup: nothing on screen is owned by a pending
                    // continuation, so drop the on-screen model (clear_history).
                    self.zchd_clear_history();
                    self.zchd_ticks_until_disable = 0;
                } else {
                    // A followup is pending: keep `zchd_on_screen` (the last
                    // activation's output plus its trailing smart space) so the
                    // followup reconciles its common prefix against it. Also keep the
                    // deadline running across the idle gap so that waiting longer than
                    // the followup deadline cancels the pending followup; otherwise it
                    // would persist until the next keypress and fire seconds later.
                    self.zchd_ticks_until_disable = followup_deadline_ticks;
                }
            }
            (ZchLastPressClassification::IsChord, false) => {
                log::debug!("some released->zippy enabled");
                self.zchd_ticks_until_disable = 0;
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ZchState {
    /// Dynamic state. Maybe doesn't make sense to separate this from zch_chords and to instead
    /// just flatten the structures.
    zchd: ZchDynamicState,
    /// Chords configured by the user. This is fixed at runtime other than live-reloads replacing
    /// the state.
    zch_chords: ZchPossibleChords,
    /// Options to configure behaviour.
    zch_cfg: ZchConfig,
}

impl ZchState {
    /// Configure zippychord behaviour.
    pub(crate) fn zch_configure(&mut self, cfg: (ZchPossibleChords, ZchConfig)) {
        self.zch_chords = cfg.0;
        self.zch_cfg = cfg.1;
        self.zchd.zchd_reset();
    }

    /// Zch handling for key presses.
    pub(crate) fn zch_press_key(
        &mut self,
        kb: &mut KbdOut,
        osc: OsCode,
    ) -> Result<(), std::io::Error> {
        if self.zch_chords.is_empty() {
            return kb.press_key(osc);
        }
        // The suppress-space key is a flag that does not participate in chord
        // matching, only recording its held state (like altgr). It passes
        // through with its normal output, so choose a key whose passthrough is
        // acceptable while chording.
        if let ZchSuppressSpaceCfg::Key(k) = self.zch_cfg.zch_cfg_suppress_space
            && osc == k
        {
            self.zchd.zchd_is_suppress_space_active = true;
            return kb.press_key(osc);
        }
        match osc {
            OsCode::KEY_LEFTSHIFT => {
                self.zchd.zchd_is_lsft_active = true;
                return kb.press_key(osc);
            }
            OsCode::KEY_RIGHTSHIFT => {
                self.zchd.zchd_is_rsft_active = true;
                return kb.press_key(osc);
            }
            OsCode::KEY_RIGHTALT => {
                self.zchd.zchd_is_altgr_active = true;
                return kb.press_key(osc);
            }
            osc if osc.is_zippy_ignored() => {
                return kb.press_key(osc);
            }
            _ => {}
        }
        if self.zchd.zchd_smart_space_state == ZchSmartSpaceState::Sent
            && self
                .zch_cfg
                .zch_cfg_smart_space_punctuation
                .contains(&match (
                    self.zchd.zchd_is_lsft_active | self.zchd.zchd_is_rsft_active,
                    self.zchd.zchd_is_altgr_active,
                ) {
                    (false, false) => ZchOutput::Lowercase(osc),
                    (true, false) => ZchOutput::Uppercase(osc),
                    (false, true) => ZchOutput::AltGr(osc),
                    (true, true) => ZchOutput::ShiftAltGr(osc),
                })
        {
            self.zchd.zchd_characters_to_delete_on_next_activation -= 1;
            // The auto-erased trailing smart space is the last thing on screen; drop
            // it from the on-screen model too so the prefix optimization stays accurate.
            self.zchd.zchd_on_screen.pop();
            kb.press_key(OsCode::KEY_BACKSPACE)?;
            kb.release_key(OsCode::KEY_BACKSPACE)?;
        }
        self.zchd.zchd_smart_space_state = ZchSmartSpaceState::Inactive;
        if self.zchd.zchd_enabled_state != ZchEnabledState::Enabled {
            return kb.press_key(osc);
        }

        // Zippychording is enabled. Ensure the deadline to disable it if no chord activates is
        // active. A pending followup (a chord already activated and is awaiting its continuation)
        // uses the more forgiving followup deadline; otherwise this is a fresh initial chord.
        let press_deadline = if self.zchd.zchd_prioritized_chords.is_some() {
            self.zch_cfg.zch_cfg_ticks_followup_deadline
        } else {
            self.zch_cfg.zch_cfg_ticks_chord_deadline
        };
        self.zchd.zchd_activate_chord_deadline(press_deadline);
        self.zchd.zchd_state_change(&self.zch_cfg);
        self.zchd.zchd_press_key(osc);

        // There might be an activation.
        // - delete typed keys
        // - output activation
        //
        // Key deletion needs to remove typed keys as well as past activations that need to be
        // cleaned up, e.g. either the antecedent in a "combo chord" or an eagerly-activated
        // chord using fewer keys, but user has still held that chord and pressed further keys,
        // activating a chord with the same+extra keys.
        let mut activation = Neither;
        if let Some(pchords) = &self.zchd.zchd_prioritized_chords {
            activation = pchords
                .lock()
                .0
                .ssm_get_or_is_subset_ksorted(self.zchd.zchd_input_keys.zchik_keys());
        }
        let is_prioritized_activation = matches!(activation, HasValue(..));
        if !is_prioritized_activation {
            let from_main = self
                .zch_chords
                .0
                .ssm_get_or_is_subset_ksorted(self.zchd.zchd_input_keys.zchik_keys());
            // Only let the main-chord lookup override a prioritized result when it
            // is at least as strong. Otherwise a multi-key followup's partial
            // (`IsSubset`) match would be clobbered by a `Neither` from the main
            // chords — soft-resetting away the pending followup so it can never
            // accumulate its remaining keys (single-key followups dodge this by
            // returning `HasValue` on the first press).
            if matches!(from_main, HasValue(..)) || matches!(activation, Neither) {
                activation = from_main;
            }
        }

        // Martin's parse preference: if the keys held now, together with those a
        // provisional followup consumed, complete a main chord, that parse orphans no
        // key and beats the followup-plus-orphans one. Checked before the plain
        // activation below because it consumes strictly more of what was pressed.
        if !is_prioritized_activation && let Some(undo) = self.zchd.zchd_followup_undo.take() {
            let mut union = self.zchd.zchd_input_keys.clone();
            for k in undo.keys.iter().copied() {
                union.zchik_insert(OsCode::from(k));
            }
            if let HasValue(orphan_free) = self
                .zch_chords
                .0
                .ssm_get_or_is_subset_ksorted(union.zchik_keys())
            {
                return self.zch_correct_to_orphan_free(kb, undo, orphan_free, union);
            }
            self.zchd.zchd_followup_undo = Some(undo);
        }

        match activation {
            HasValue(a) => {
                // Over-eager guard: if the held keys are also a proper subset of a
                // longer chord that could still be completed, do not expand yet. Echo
                // the key and remember this complete chord as `deferred`; it fires on
                // release / deadline only if nothing longer supersedes it. This is
                // what prevents the visible error->response->sure churn.
                //
                // A chord with any *no-erase* output is exempt: no-erase is the
                // deliberate "persistent prefix" mechanism — its characters are meant
                // to be deposited eagerly and survive the next activation (e.g. a dead
                // key typed by the shorter chord and kept by the longer one), so
                // deferring it would defeat the feature, not remove churn.
                let is_eager_prefix = a.zch_output.iter().any(|o| o.osc_and_is_noerase().1);
                let has_strict_superset = !is_prioritized_activation
                    && !a.zch_output.is_empty()
                    && !is_eager_prefix
                    && self
                        .zch_chords
                        .0
                        .ssm_has_strict_superset_ksorted(self.zchd.zchd_input_keys.zchik_keys());
                if has_strict_superset {
                    self.zchd.zchd_deferred = Some(a);
                    self.zch_echo_key(kb, osc)
                } else {
                    self.zch_activate(
                        kb,
                        a,
                        is_prioritized_activation,
                        Some(osc),
                        ZchTrailingSpace::Decide,
                    )
                }
            }
            IsSubset => {
                // The held keys are no longer a complete chord; drop any deferral.
                self.zchd.zchd_deferred = None;
                self.zch_echo_key(kb, osc)
            }
            Neither => {
                self.zchd.zchd_soft_reset();
                kb.press_key(osc)
            }
        }
    }

    /// Put back the word a provisional followup replaced and type the main chord the
    /// held keys complete, so no pressed key is left orphaned. Reuses the ordinary
    /// activation path twice: re-activating the replaced chord rewrites the followup's
    /// word back to it (the overlap path already computes those deletes), and zeroing
    /// the delete counter in between commits it, exactly as a full key release would,
    /// so the main chord then appends instead of replacing.
    fn zch_correct_to_orphan_free(
        &mut self,
        kb: &mut KbdOut,
        undo: ZchFollowupUndo,
        orphan_free: Arc<ZchChordOutput>,
        union: ZchInputKeys,
    ) -> Result<(), std::io::Error> {
        // Both activations below type through `type_osc`, which must know every key the
        // user is physically holding — including the ones the followup consumed, which
        // were cleared from the input set — or it presses a held key a second time.
        self.zchd.zchd_input_keys = union;
        // The restore must erase the followup's own word on top of anything echoed
        // since: keys pressed before the main chord completed are on screen too.
        self.zchd.zchd_characters_to_delete_on_next_activation +=
            self.zchd.zchd_prior_activation_output_count;
        self.zch_activate(
            kb,
            undo.replaced,
            false,
            None,
            ZchTrailingSpace::Replay(undo.added_trailing_space),
        )?;
        // The restored word is committed, not a region the next activation may rewrite:
        // clearing the on-screen model is what a full key release would do, and without
        // it the common-prefix reuse would suppress an output that merely repeats it.
        self.zchd.zchd_characters_to_delete_on_next_activation = 0;
        self.zchd.zchd_on_screen.clear();
        self.zchd.zchd_echo_len = 0;
        self.zchd.zchd_first_pressed_key = undo.first_pressed;
        self.zch_activate(kb, orphan_free, false, None, ZchTrailingSpace::Decide)
    }

    /// Echo an input key through while a chord is still being formed — the held keys
    /// are a strict subset/prefix of a chord, or a complete-but-`deferred` chord.
    /// Tracks the key in the on-screen model so a later activation can reuse the common
    /// prefix it shares with the output instead of backspacing and retyping.
    fn zch_echo_key(&mut self, kb: &mut KbdOut, osc: OsCode) -> Result<(), std::io::Error> {
        self.zchd.zchd_last_press = ZchLastPressClassification::NotChord;
        self.zchd.zchd_characters_to_delete_on_next_activation += 1;
        let echoed = match (
            self.zchd.zchd_is_lsft_active | self.zchd.zchd_is_rsft_active,
            self.zchd.zchd_is_altgr_active,
        ) {
            (false, false) => ZchOutput::Lowercase(osc),
            (true, false) => ZchOutput::Uppercase(osc),
            (false, true) => ZchOutput::AltGr(osc),
            (true, true) => ZchOutput::ShiftAltGr(osc),
        };
        self.zchd.zchd_on_screen.push(echoed);
        self.zchd.zchd_echo_len += 1;
        kb.press_key(osc)
    }

    /// Perform a chord activation: delete the eagerly-shown characters this output
    /// replaces (reusing the common prefix with the on-screen model) and type the
    /// output, then add the trailing smart space. `osc` is the triggering key press,
    /// or `None` when this is a `deferred` chord fired from a release / deadline.
    fn zch_activate(
        &mut self,
        kb: &mut KbdOut,
        a: Arc<ZchChordOutput>,
        is_prioritized_activation: bool,
        osc: Option<OsCode>,
        trailing: ZchTrailingSpace,
    ) -> Result<(), std::io::Error> {
        // Any activation supersedes a pending deferred chord.
        self.zchd.zchd_deferred = None;
        // Reuse the longest prefix this activation's output shares with what is
        // already on screen (`zchd_on_screen` — echoed input keys, the prior
        // activation's output, and any trailing smart space). This drives both
        // the number of backspaces and the number of characters re-typed, so
        // any already-correct character (an echoed key the output extends, a
        // prior overlapping output, an aligned space) is kept rather than
        // backspaced and re-typed. `on_screen_space_after_prefix` reports
        // whether the on-screen char just past that prefix is a space, so this
        // activation's own trailing smart space can land on it instead of a
        // delete+retype (see `preserve_trailing_space`).
        let common_prefix_len_from_past_activation =
            common_output_prefix_len(&self.zchd.zchd_on_screen, &a.zch_output);
        let on_screen_space_after_prefix = self
            .zchd
            .zchd_on_screen
            .get(common_prefix_len_from_past_activation as usize)
            .map(|out| out.osc() == OsCode::KEY_SPACE)
            .unwrap_or(false);

        // After an activation the window is held open only to catch a followup, so when this
        // chord has followups use the followup deadline. (This is the held-continuation path
        // where keys are not released between chords; the release-then-press path re-arms via
        // `zchd_activate_chord_deadline` above.)
        let restart_deadline = if a.zch_followups.is_some() {
            self.zch_cfg.zch_cfg_ticks_followup_deadline
        } else {
            self.zch_cfg.zch_cfg_ticks_chord_deadline
        };
        self.zchd.zchd_restart_deadline(restart_deadline);

        // Whether the user wants to suppress the trailing smart space
        // for this activation. `Key` mode suppresses while the flag key
        // is held; `NotFirstDefKey` suppresses unless the first key the
        // user pressed is the chord definition's first key.
        let suppress_space = match self.zch_cfg.zch_cfg_suppress_space {
            ZchSuppressSpaceCfg::Disabled => false,
            ZchSuppressSpaceCfg::Key(_) => self.zchd.zchd_is_suppress_space_active,
            ZchSuppressSpaceCfg::NotFirstDefKey => matches!(
                (self.zchd.zchd_first_pressed_key, a.zch_first_def_key),
                (Some(first), Some(def)) if first != def
            ),
        };
        // Whether this activation appends a trailing smart space: smart
        // space enabled, not suppressed, and the output ends in a normal
        // (non-space, non-backspace) character. Computed up front because
        // it feeds the common-prefix optimization below.
        let adds_smart_space = match trailing {
            ZchTrailingSpace::Replay(had_space) => had_space,
            ZchTrailingSpace::Decide => {
                !suppress_space
                    && self.zch_cfg.zch_cfg_smart_space != ZchSmartSpaceCfg::Disabled
                    && a.zch_output
                        .last()
                        .map(|out| !matches!(out.osc(), OsCode::KEY_SPACE | OsCode::KEY_BACKSPACE))
                        .unwrap_or(false /* if output is empty, don't do smart spacing */)
            }
        };
        // The trailing smart space is emitted separately from `zch_output`,
        // so the prefix run above stops at the word. When this activation's
        // word is a full prefix of what is on screen and the on-screen text
        // already has a space at that position, that space *is* the trailing
        // smart space — keep it rather than backspacing and re-typing an
        // identical one. (`no_redundant_prefix_delete` PBT invariant.)
        let preserve_trailing_space = adds_smart_space
            && common_prefix_len_from_past_activation as usize == a.zch_output.len()
            && on_screen_space_after_prefix;

        if !a.zch_output.is_empty() {
            // Zippychording eagerly types characters that form a chord and also eagerly
            // outputs chords that are of a maybe-to-be-activated-later chord with more
            // participating keys. This procedure erases both classes of typed characters
            // in order to have the correct typed output for this chord activation.
            for _ in 0..(self.zchd.zchd_characters_to_delete_on_next_activation
                + if is_prioritized_activation {
                    self.zchd.zchd_prior_activation_output_count
                } else {
                    0
                }
                - common_prefix_len_from_past_activation
                - i16::from(preserve_trailing_space))
            {
                kb.press_key(OsCode::KEY_BACKSPACE)?;
                kb.release_key(OsCode::KEY_BACKSPACE)?;
            }
            // The common-prefix optimization left `common_prefix_len`
            // characters of this activation's output on screen (they were
            // not re-typed). They are still part of the visible output and
            // must be counted for deletion by the next activation; the
            // typing loop below only re-accumulates the freshly typed
            // (skipped-past-prefix) characters, so seed the counter with
            // the kept prefix length instead of zeroing it.
            self.zchd.zchd_characters_to_delete_on_next_activation =
                common_prefix_len_from_past_activation;
            self.zchd.zchd_prior_activation_output_count = ZchOutput::display_len(&a.zch_output);
            // The activation deleted everything past the shared prefix and
            // (re)typed the full output, so that output is exactly what is now
            // on screen. A trailing smart space, if added, is appended below.
            self.zchd.zchd_on_screen = a.zch_output.to_vec();
            self.zchd.zchd_echo_len = 0;
        } else {
            // Followup chords may consist of an empty output; eventually in the followup
            // chain has an activation output that is not empty. For empty outputs, do not
            // do any backspacing. A deferred chord is never empty (the over-eager guard
            // requires a non-empty output), so this branch always has a triggering key.
            let osc = osc.expect("empty-output activation only happens on a key press");
            self.zchd.zchd_characters_to_delete_on_next_activation += 1;
            self.zchd.zchd_prior_activation_output_count +=
                self.zchd.zchd_input_keys.zchik_keys().len() as i16;
            // The input key is echoed through (not an output replacement), so it
            // is appended to what is on screen.
            self.zchd.zchd_on_screen.push(ZchOutput::Lowercase(osc));
            self.zchd.zchd_echo_len += 1;
            kb.press_key(osc)?;
        }

        self.zchd
            .zchd_prioritized_chords
            .clone_from(&a.zch_followups);
        let mut released_sft = false;
        #[cfg(feature = "interception_driver")]
        let mut send_count = 0;
        if self.zchd.zchd_is_altgr_active && !a.zch_output.is_empty() {
            kb.release_key(OsCode::KEY_RIGHTALT)?;
        }
        for key_to_send in a
            .zch_output
            .iter()
            .copied()
            .skip(common_prefix_len_from_past_activation as usize)
        {
            #[cfg(feature = "interception_driver")]
            {
                // Note: every 5 keys on Windows Interception, do a sleep because
                // sending too quickly apparently causes weird behaviour...
                // I guess there's some buffer in the Interception code that is filling up.
                send_count += 1;
                if send_count % 5 == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }

            match key_to_send {
                ZchOutput::Lowercase(osc) | ZchOutput::NoEraseLowercase(osc) => {
                    type_osc(osc, kb, &self.zchd)?;
                }
                ZchOutput::Uppercase(osc) | ZchOutput::NoEraseUppercase(osc) => {
                    maybe_press_sft_during_activation(released_sft, kb, &self.zchd)?;
                    type_osc(osc, kb, &self.zchd)?;
                    maybe_release_sft_during_activation(released_sft, kb, &self.zchd)?;
                }
                ZchOutput::AltGr(osc) | ZchOutput::NoEraseAltGr(osc) => {
                    // A note regarding maybe_press|release_sft
                    // in contrast to always pressing|releasing altgr:
                    //
                    // The maybe-logic is valuable with Shift to capitalize the first
                    // typed output during activation.
                    // However, altgr - if already held -
                    // does not seem useful to keep held on the first typed output so it is
                    // always released at the beginning and pressed at the end if it was
                    // previously being held.
                    kb.press_key(OsCode::KEY_RIGHTALT)?;
                    type_osc(osc, kb, &self.zchd)?;
                    kb.release_key(OsCode::KEY_RIGHTALT)?;
                }
                ZchOutput::ShiftAltGr(osc) | ZchOutput::NoEraseShiftAltGr(osc) => {
                    kb.press_key(OsCode::KEY_RIGHTALT)?;
                    maybe_press_sft_during_activation(released_sft, kb, &self.zchd)?;
                    type_osc(osc, kb, &self.zchd)?;
                    maybe_release_sft_during_activation(released_sft, kb, &self.zchd)?;
                    kb.release_key(OsCode::KEY_RIGHTALT)?;
                }
            };

            self.zchd.zchd_characters_to_delete_on_next_activation +=
                key_to_send.output_char_count();

            if !released_sft && !self.zchd.zchd_is_caps_word_active {
                released_sft = true;
                if self.zchd.zchd_is_lsft_active {
                    kb.release_key(OsCode::KEY_LEFTSHIFT)?;
                }
                if self.zchd.zchd_is_rsft_active {
                    kb.release_key(OsCode::KEY_RIGHTSHIFT)?;
                }
            }
        }

        // A suppressed trailing space is behaviorally identical to
        // `smart-space` being disabled: the whole block below (including
        // the eager participating-space release) is skipped, leaving any
        // eager space held until its physical release, exactly as the
        // smart-space-disabled path does. (`suppress_space` /
        // `adds_smart_space` are computed before the delete loop above.)
        if adds_smart_space {
            if self.zch_cfg.zch_cfg_smart_space == ZchSmartSpaceCfg::Full {
                self.zchd.zchd_smart_space_state = ZchSmartSpaceState::Sent;
            }

            // It might look unusual to add to both.
            // This is correct to do.
            // zchd_prior_activation_output_count only applies to followup activations,
            // which should only occur after a full release+repress of a new chord.
            // The full release will set zchd_characters_to_delete_on_next_activation to 0.
            // Overlapping chords do not use zchd_prior_activation_output_count but
            // instead keep track of characters to delete via
            // zchd_characters_to_delete_on_next_activation,
            // which is incremented both by typing characters
            // to achieve a chord in the first place,
            // as well as by chord activations that are overlapped
            // by the intended final chord.
            //
            // These counters track the trailing space whether it was freshly
            // typed or preserved from the on-screen prefix, since either way
            // it is on screen and owned by this activation.
            self.zchd.zchd_prior_activation_output_count += 1;
            self.zchd.zchd_characters_to_delete_on_next_activation += 1;

            // The participating space of a leading-space chord (e.g. " n"
            // -> "no") is typed eagerly as a Space press and left held. If
            // it is still held when the smart space is added, the output
            // would contain two Space-downs with no Space-up between them;
            // a real OS coalesces those into one held key and — since the
            // eager space's character was already backspaced — the trailing
            // smart space is silently dropped (user sees "no" not "no ").
            // Release the held participating space first. Releasing a space
            // that isn't held (chord activated letter-first) is a harmless
            // OS no-op, the same pattern type_osc already relies on. The
            // smart space itself stays a clean tap so holding the physical
            // space does not auto-repeat it.
            if self.zchd.zchd_input_keys.zchik_contains(OsCode::KEY_SPACE) {
                kb.release_key(OsCode::KEY_SPACE)?;
            }
            // Skip the tap entirely when the trailing space was preserved
            // from the on-screen prefix (it is already there) — that is the
            // redundant delete-then-retype this optimization removes.
            if !preserve_trailing_space {
                kb.press_key(OsCode::KEY_SPACE)?;
                kb.release_key(OsCode::KEY_SPACE)?;
            }
            // Either way the trailing smart space is now on screen (the output
            // was set above without it), so record it for the next activation's
            // prefix reuse.
            self.zchd
                .zchd_on_screen
                .push(ZchOutput::Lowercase(OsCode::KEY_SPACE));
        }

        // Everything above must leave `zchd_on_screen` describing exactly what this
        // module has on screen, because the next activation's delete budget is counted
        // from it. An empty output echoes its key through instead of replacing the
        // region, so it keeps whatever was already there.
        if !a.zch_output.is_empty() {
            assert_eq!(
                self.zchd.zchd_characters_to_delete_on_next_activation,
                ZchOutput::display_len(&self.zchd.zchd_on_screen),
                "on-screen model {:?} disagrees with the delete budget",
                self.zchd.zchd_on_screen
            );
        }

        if !self.zchd.zchd_is_caps_word_active {
            // When expanding, lsft/rsft will be released after the first press.
            if self.zchd.zchd_is_lsft_active {
                kb.press_key(OsCode::KEY_LEFTSHIFT)?;
            }
            if self.zchd.zchd_is_rsft_active {
                kb.press_key(OsCode::KEY_RIGHTSHIFT)?;
            }
        }
        if self.zchd.zchd_is_altgr_active && !a.zch_output.is_empty() {
            kb.press_key(OsCode::KEY_RIGHTALT)?;
        }

        // A plain chord keeps its input keys: holding them and pressing one more may
        // complete an overlapping chord that supersedes this output, which is why
        // zippychord may expand eagerly. E.g. with `ab` => Abba and `abc` => Alphabet,
        // typing (b a) outputs "Abba"; holding those and pressing (c) erases it and
        // outputs "Alphabet".
        //
        // A followup instead ends a word, so its keys leave the input set and a chain
        // link (`do s` => `do s n`) accumulates from empty. They move to
        // `zchd_followup_undo` rather than being forgotten: while they are still held
        // they can complete a main chord, and that parse orphans no key, so it wins.
        // Emptying the input set makes the next followup reachable without the full
        // release that would otherwise have zeroed this counter, and what is on screen
        // is owned by `zchd_prior_activation_output_count` alone; leaving it set would
        // double-count the word and over-delete.
        if is_prioritized_activation {
            self.zchd.zchd_followup_undo =
                self.zchd
                    .zchd_last_activation
                    .take()
                    .map(|replaced| ZchFollowupUndo {
                        replaced,
                        keys: self.zchd.zchd_input_keys.zchik_keys().to_vec(),
                        first_pressed: self.zchd.zchd_first_pressed_key,
                        added_trailing_space: self.zchd.zchd_last_activation_added_space,
                    });
            self.zchd.zchd_input_keys.zchik_clear();
            self.zchd.zchd_characters_to_delete_on_next_activation = 0;
        }
        self.zchd.zchd_last_activation = Some(a.clone());
        self.zchd.zchd_last_activation_added_space = adds_smart_space;

        self.zchd.zchd_last_press = ZchLastPressClassification::IsChord;
        Ok(())
    }

    // Zch handling for key releases.
    pub(crate) fn zch_release_key(
        &mut self,
        kb: &mut KbdOut,
        osc: OsCode,
    ) -> Result<(), std::io::Error> {
        if self.zch_chords.is_empty() {
            return kb.release_key(osc);
        }
        if let ZchSuppressSpaceCfg::Key(k) = self.zch_cfg.zch_cfg_suppress_space
            && osc == k
        {
            self.zchd.zchd_is_suppress_space_active = false;
            return kb.release_key(osc);
        }
        match osc {
            OsCode::KEY_LEFTSHIFT => {
                self.zchd.zchd_is_lsft_active = false;
            }
            OsCode::KEY_RIGHTSHIFT => {
                self.zchd.zchd_is_rsft_active = false;
            }
            OsCode::KEY_RIGHTALT => {
                self.zchd.zchd_is_altgr_active = false;
            }
            _ => {}
        }
        if osc.is_zippy_ignored() {
            return kb.release_key(osc);
        }
        // Releasing one of a deferred chord's keys commits to that chord (the user
        // stopped extending it), so fire it now — while all its keys are still held,
        // before the release is processed. Only a held chord key counts; a modifier
        // release keeps the chord deferred.
        if self.zchd.zchd_input_keys.zchik_contains(osc)
            && let Some(a) = self.zchd.zchd_deferred.take()
        {
            self.zch_activate(kb, a, false, None, ZchTrailingSpace::Decide)?;
        }
        // Releasing a key a provisional followup consumed ends the alternative parse:
        // that key can no longer join a main chord, so the followup's word stands.
        if self
            .zchd
            .zchd_followup_undo
            .as_ref()
            .is_some_and(|u| u.keys.contains(&u16::from(osc)))
        {
            self.zchd.zchd_followup_undo = None;
        }
        self.zchd.zchd_state_change(&self.zch_cfg);
        self.zchd
            .zchd_release_key(osc, self.zch_cfg.zch_cfg_ticks_followup_deadline);
        kb.release_key(osc)
    }

    /// Tick the zch output state. When the chord deadline elapses with a deferred
    /// chord pending, fire it here (this path has keyboard-output access).
    pub(crate) fn zch_tick(
        &mut self,
        kb: &mut KbdOut,
        is_caps_word_active: bool,
        layout_pending: bool,
    ) -> Result<(), std::io::Error> {
        if self.zchd.zchd_tick(is_caps_word_active, layout_pending)
            && let Some(a) = self.zchd.zchd_deferred.take()
        {
            self.zch_activate(kb, a, false, None, ZchTrailingSpace::Decide)?;
        }
        Ok(())
    }

    /// Returns true if zch state has no further processing so the idling optimization can
    /// activate.
    pub(crate) fn zch_is_idle(&self) -> bool {
        self.zchd.zchd_is_idle()
    }
}

/// Longest common prefix length (in output entries) between what is currently on
/// screen (`on_screen`) and a new activation's output. A backspace entry on either
/// side stops the run: backspaced characters are not safe to treat as reusable
/// prefix. Used to avoid backspacing and retyping characters that are already correct
/// — both the echoed input keys (first activation) and a prior activation's output
/// (overlapping activations).
fn common_output_prefix_len(on_screen: &[ZchOutput], output: &[ZchOutput]) -> i16 {
    let mut len: i16 = 0;
    for (past, current) in on_screen.iter().copied().zip(output.iter().copied()) {
        if past.osc() == OsCode::KEY_BACKSPACE
            || current.osc() == OsCode::KEY_BACKSPACE
            || past != current
        {
            break;
        }
        len += 1;
    }
    len
}

fn type_osc(osc: OsCode, kb: &mut KbdOut, zchd: &ZchDynamicState) -> Result<(), std::io::Error> {
    if zchd.zchd_input_keys.zchik_contains(osc) {
        kb.release_key(osc)?;
        kb.press_key(osc)?;
    } else {
        kb.press_key(osc)?;
        kb.release_key(osc)?;
    }
    Ok(())
}

fn maybe_press_sft_during_activation(
    sft_already_released: bool,
    kb: &mut KbdOut,
    zchd: &ZchDynamicState,
) -> Result<(), std::io::Error> {
    if !zchd.zchd_is_caps_word_active
        && (sft_already_released || !zchd.zchd_is_lsft_active && !zchd.zchd_is_rsft_active)
    {
        kb.press_key(OsCode::KEY_LEFTSHIFT)?;
    }
    Ok(())
}

fn maybe_release_sft_during_activation(
    sft_already_released: bool,
    kb: &mut KbdOut,
    zchd: &ZchDynamicState,
) -> Result<(), std::io::Error> {
    if !zchd.zchd_is_caps_word_active
        && (sft_already_released || !zchd.zchd_is_lsft_active && !zchd.zchd_is_rsft_active)
    {
        kb.release_key(OsCode::KEY_LEFTSHIFT)?;
    }
    Ok(())
}
