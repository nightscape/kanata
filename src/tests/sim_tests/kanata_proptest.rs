//! Stateful property tests for Kanata, using `proptest-state-machine`.
//!
//! This file is growing from a zippychord-only PBT into a general harness that
//! hosts one feature module per Kanata construct. It currently holds three
//! state machines plus a shared invariant catalog:
//!
//! - `zippychord_state_machine` (`KanataRef`/`KanataModel`/`Sut`) — the COUPLED
//!   model. The reference generates a config + chord dictionary AND a layout with
//!   optional tap-hold keys, then a transition stream; the SUT is a real `Kanata`.
//!   The oracle is split: a `ChordExpansion` transition *carries which chord it
//!   activates* (expansion known by construction), while the reference supplies
//!   only *placement* (fresh append / followup replace / disabled passthrough)
//!   from coarse engine state. A tap-hold key's resolved tap/hold output feeds the
//!   SAME zippy enable-state + visible buffer (`type_literal`) — the cross-cutting
//!   coupling. The reference deliberately does NOT reimplement the keystroke-level
//!   eager/overlap/backspace accounting NOR the tap-hold-vs-deadline arbitration —
//!   that is the code under test — so those bugs surface as a mismatch. (This is
//!   how the common-prefix backspace under-count bug was found.) The config also
//!   varies `suppress-space not-first-def-key`: the trailing smart space is then a
//!   pure function of the (already shuffled) press order vs the chord's first
//!   definition key, so the placement oracle predicts it exactly. (The `(key X)`
//!   suppress mode is a held flag orthogonal to press order — deferred here,
//!   covered by the deterministic sim tests.) Observable: net visible text.
//! - `taphold_state_machine` (`ThRef`/`ThModel`/`ThSut`) — tap-hold in ISOLATION,
//!   a pure construction oracle on the lower-level **event stream** (`event_seq`),
//!   with tap output ≠ input. Kept as a fast/isolated slice because it provides an
//!   observable and a coverage region (tap≠self) the coupled net-text model cannot.
//! - `interaction_taphold_zippy_order_independent` — the chord×tap-hold overlap
//!   (a key that is both), judged by a metamorphic determinism oracle. GREEN since
//!   the press-order bug was fixed (zippychord freezes its chord deadline while the
//!   layout is still deferring a tap-hold decision; see `zchd_tick`). The coupled
//!   model does NOT generate this overlap (chord keys and tap-hold inputs are
//!   disjoint alphabets).
//!
//! Coverage / deferred dimensions and the oracle/decomposition design are tracked
//! in ZIPPY_PBT_NOTES.md.

use crate::oskbd::{KeyEvent, KeyValue};
use crate::tests::CFG_PARSE_LOCK;
use crate::{Kanata, str_to_oscode};
use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{
    ReferenceStateMachine, StateMachineTest, prop_state_machine_persisted,
};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::sync::MutexGuard;

// Letters that participate in chords (small alphabet => frequent overlaps).
const INPUT_ALPHA: &[char] = &['a', 'b', 'c', 'd'];
// Letters used for literal (non-chord) typing — disjoint from INPUT_ALPHA so a
// literal press is always "Neither" (disables zippy), never a chord subset.
const NONCHORD_ALPHA: &[char] = &['u', 'v', 'w', 'x', 'y', 'z'];
// Punctuation chars in zippychord's default smart-space-punctuation set that the SM
// generates as literals (to exercise smart-space `full` auto-erase). Only `.` for now
// — the one whose output key-name (`Dot`) `key_to_char`/`net_text` decode; `,`/`;`
// are deferred pending their output-name decode. Disjoint from chord (a–d), free, and
// tap-hold (u–z) alphabets, so they cannot form a chord or be a tap-hold input.
const SMART_SPACE_PUNCT: &[char] = &['.'];

fn is_smart_space_punct(c: char) -> bool {
    SMART_SPACE_PUNCT.contains(&c)
}
// Alphabet for free typing. Intentionally INCLUDES chord-participating keys
// (a-d) and space so that free typing can incidentally trigger chord
// activations — which the naive "literal append" oracle mispredicts. The PBT is
// meant to discover that; see ZIPPY_PBT_NOTES.md.
const FREE_ALPHA: &[char] = &['a', 'b', 'c', 'd', ' ', 'u', 'v', 'w'];

// Timer DEFAULTS. The SM now *generates* the timers per config (see `arb_cfg`); these
// constants are the deserialization defaults for older seeds and the fixed values the
// deterministic reference-test helper uses. `wait` (idle-reactivate-time) is large so
// the WaitEnable countdown only crosses on an explicit "full" Idle, never mid-hold;
// the initial vs followup chord deadlines are generated independently so their
// divergence is exercised rather than assumed equal.
const WAIT: u16 = 500;
const DEADLINE: u16 = 50;
// Max per-event timing gaps for a ChordExpansion gesture. The largest chord is
// INPUT_ALPHA (4) plus a leading space (5 keys), so the worst-case cumulative
// span of a press or release phase is 5 * (GAP_MAX + 1 processing tick), which
// must stay below DEADLINE so the chord is guaranteed to form. 6 => 35 < 50.
const PRESS_GAP_MAX: u16 = 6;
const RELEASE_GAP_MAX: u16 = 6;

// ---------------------------------------------------------------------------
// Dictionary model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)] // Backspace deferred (see ZIPPY_PBT_NOTES.md)
enum OutItem {
    Char(char), // already-cased net visible char (e.g. 'a', 'A', ' ')
    Backspace,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Child {
    // A followup component is symmetric with a root: an optional leading space
    // plus a multi-key set drawn from INPUT_ALPHA (e.g. `vb ig` -> `vibing`). The
    // parser only ever treats a space as the *first* key of a component (it strips
    // a leading space then ends the component at the next space), written in the
    // TSV as a double space — the ` w  a` -> `(space w),(space a)` Washington case.
    // `default` so older persisted seeds (single-key, no lead space) deserialize.
    #[serde(default)]
    lead_space: bool,
    keys: BTreeSet<char>,
    out: Vec<OutItem>,
    followups: Vec<Child>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Root {
    lead_space: bool,
    keys: BTreeSet<char>,
    out: Vec<OutItem>,
    followups: Vec<Child>,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum SmartSpace {
    None,
    AddOnly,
    Full,
}

/// `suppress-space` config, modelled for the PBT.
///
/// Only the press-order mode (`not-first-def-key`) is generated here: it is a
/// pure function of press order + dictionary, which this harness already varies
/// (`prop_shuffle`), so the placement oracle stays exact. The `(key X)` mode is
/// a held-flag orthogonal to press order and is covered by the deterministic sim
/// tests instead (threading an extra held key through every gesture would be
/// invasive for little marginal coverage). Deferred dimension; see
/// ZIPPY_PBT_NOTES.md.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
enum SuppressSpace {
    #[default]
    None,
    NotFirstDefKey,
}

// Defaults for the generated timers — chosen so older persisted seeds (which lack
// these fields) deserialize to the previously-fixed behaviour.
fn default_chord_deadline() -> u16 {
    DEADLINE
}
fn default_followup_deadline() -> u16 {
    DEADLINE
}
fn default_wait() -> u16 {
    WAIT
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct ModelCfg {
    smart_space: SmartSpace,
    // `default` so older persisted seeds (no suppress_space) still deserialize.
    #[serde(default)]
    suppress_space: SuppressSpace,
    // Generated timers (dimension lift: stop forcing the timers equal/fixed). The
    // initial and followup chord deadlines are generated *independently* so the
    // deadline arithmetic — and the followup-vs-initial divergence — is exercised
    // rather than assumed. `idle-reactivate-time` (wait) is generated too, kept
    // above the tap-hold settle (250ms) so a tap can't re-enable zippy mid-gesture.
    #[serde(default = "default_chord_deadline")]
    chord_deadline: u16,
    #[serde(default = "default_followup_deadline")]
    followup_deadline: u16,
    #[serde(default = "default_wait")]
    wait: u16,
}

// ---------------------------------------------------------------------------
// Reference state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Enabled {
    Enabled,
    WaitEnable,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KanataModel {
    cfg: ModelCfg,
    roots: Vec<Root>,
    // Tap-hold keys in the layout (coupled feature). Their resolved output feeds
    // the SAME zippy engine state below — the cross-cutting coupling. `default`
    // so older persisted seeds (no tap-hold) still deserialize as empty.
    #[serde(default)]
    taphold: Vec<ThKey>,
    // dynamic coarse engine state:
    enabled: Enabled,
    until_enabled: u16,
    visible: Vec<char>,
    prioritized: Option<Vec<Child>>,
    last_act_len: usize, // visible chars the last activation owns at the tail
    smart_space_sent: bool,
    // Followup-deadline budget (dimension 12). `Some(ticks)` exactly when a
    // followup is pending: armed to the followup deadline (= DEADLINE, since the SM
    // never sets `followup-chord-deadline`) when `prioritized` becomes `Some`, and
    // counted down by `Idle`. On reaching 0 the followup is cancelled but zippy
    // stays Enabled — the cleared-but-enabled state. `default` so older persisted
    // seeds still deserialize. Kept in lockstep with `prioritized.is_some()`.
    #[serde(default)]
    followup_until_clear: Option<u16>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Target {
    Root(usize),
    Followup(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyAction {
    Press(char),
    Release(char),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum KanataTransition {
    /// A gesture that activates `target`. The keystrokes are a granular timed
    /// stream: each `(delay_ms, action)` ticks `delay_ms` *before* applying the
    /// action. The generator is "smart" — it emits every target key as a press
    /// (in arbitrary order, with arbitrary inter-press timing that stays inside
    /// the chord deadline) *before* any release, so all keys are simultaneously
    /// held when the last press lands and the target chord is guaranteed to fire.
    /// That keeps the oracle exact (the activation is known by construction) while
    /// still exercising the timing/ordering-dependent eager-activation paths where
    /// the backspace accounting bugs live.
    ChordExpansion {
        target: Target,
        events: Vec<(u16, KeyAction)>,
    },
    Literal {
        key: char,
    },
    /// Tap the `i`th tap-hold key (release before the hold timeout). Its tap
    /// output is a non-chord key, so — like a `Literal` — it types that char and
    /// disables zippy. Exercises the layout→zippy coupling.
    TapHoldTap(usize),
    /// Hold the `i`th tap-hold key past the hold timeout. Emits its hold output
    /// (also a non-chord key) and disables zippy.
    TapHoldHold(usize),
    Idle {
        ms: u16,
    },
    /// Free typing: hold an arbitrary set of keys (press order / release order
    /// shuffled), NOT targeted to any chord. The reference predicts naive literal
    /// append (treats them as ordinary keystrokes).
    FreeType {
        press: Vec<char>,
        release: Vec<char>,
    },
}

impl KanataTransition {
    /// Keys pressed by a `ChordExpansion`, in press order.
    fn press_order(events: &[(u16, KeyAction)]) -> Vec<char> {
        events
            .iter()
            .filter_map(|(_, a)| match a {
                KeyAction::Press(c) => Some(*c),
                KeyAction::Release(_) => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Serialization to a zippy config + chord file
// ---------------------------------------------------------------------------

fn out_to_tsv(out: &[OutItem]) -> String {
    out.iter()
        .map(|i| match i {
            OutItem::Char(c) => *c,
            OutItem::Backspace => '⌫',
        })
        .collect()
}

impl KanataModel {
    fn cfg_string(&self) -> String {
        let ss = match self.cfg.smart_space {
            SmartSpace::None => "none",
            SmartSpace::AddOnly => "add-space-only",
            SmartSpace::Full => "full",
        };
        let suppress = match self.cfg.suppress_space {
            SuppressSpace::None => String::new(),
            SuppressSpace::NotFirstDefKey => " suppress-space not-first-def-key".to_string(),
        };
        // Tap-hold keys join the layout; chord keys remain unmapped passthrough
        // (zippychord reads the layout's output). tap output = the key itself so a
        // tap is an ordinary keystroke; hold output is a distinct non-chord key.
        let mut src = String::from("lalt");
        let mut lay = String::from("lalt");
        for k in &self.taphold {
            src.push(' ');
            src.push(k.input);
            lay.push_str(&format!(
                " (tap-hold {TH_TAP_TIMEOUT} {TH_HOLD_TIMEOUT} {} {})",
                k.tap_out, k.hold_out
            ));
        }
        format!(
            "(defsrc {src})(deflayer base {lay})(defzippy file \
             on-first-press-chord-deadline {} followup-chord-deadline {} \
             idle-reactivate-time {} smart-space {ss}{suppress})",
            self.cfg.chord_deadline, self.cfg.followup_deadline, self.cfg.wait
        )
    }

    fn tsv(&self) -> String {
        let mut lines = Vec::new();
        for r in &self.roots {
            let input: String = {
                let mut s = String::new();
                if r.lead_space {
                    s.push(' ');
                }
                s.extend(r.keys.iter());
                s
            };
            lines.push(format!("{input}\t{}", out_to_tsv(&r.out)));
            emit_children(&r.followups, &input, &mut lines);
        }
        format!("\n{}\n", lines.join("\n"))
    }
}

/// Union of every key used by any chord (root keys + leading space + all
/// followup keys, recursively). Free typing must avoid all of these.
fn chord_keys(roots: &[Root]) -> BTreeSet<char> {
    fn collect(children: &[Child], s: &mut BTreeSet<char>) {
        for c in children {
            s.extend(c.keys.iter().copied());
            if c.lead_space {
                s.insert(' ');
            }
            collect(&c.followups, s);
        }
    }
    let mut s = BTreeSet::new();
    for r in roots {
        s.extend(r.keys.iter().copied());
        if r.lead_space {
            s.insert(' ');
        }
        collect(&r.followups, &mut s);
    }
    s
}

/// Full chord key-set of a followup component: its keys plus the leading space.
fn child_chord(c: &Child) -> BTreeSet<char> {
    let mut k = c.keys.clone();
    if c.lead_space {
        k.insert(' ');
    }
    k
}

/// Full chord key-set of a root: its keys plus the leading space.
fn root_chord(r: &Root) -> BTreeSet<char> {
    let mut k = r.keys.clone();
    if r.lead_space {
        k.insert(' ');
    }
    k
}

/// Proper, non-empty subsets of `set` (excludes the empty set and `set` itself).
fn proper_subsets(set: &BTreeSet<char>) -> Vec<BTreeSet<char>> {
    let elems: Vec<char> = set.iter().copied().collect();
    let n = elems.len();
    let full = (1u32 << n) - 1;
    (1..full)
        .map(|mask| {
            (0..n)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| elems[i])
                .collect()
        })
        .collect()
}

/// Drop followups the placement oracle cannot model. A multi-key followup is
/// pressed key-by-key (in shuffled order), so its formation transiently *holds*
/// every proper subset of its keys. If such a subset exactly matches a sibling
/// followup or a root, that chord activates eagerly mid-press — changing which
/// followups are pending (or firing a fresh root) — and the gesture no longer
/// resolves to the intended followup. The oracle assumes the target forms
/// atomically, so these cases are excluded. Single-key followups have no proper
/// subset and are always kept. (A subset that is merely a *partial* match of the
/// pending followup stays `IsSubset` and keeps accumulating — that is fine; only
/// exact matches of a smaller chord break atomic formation.) Deferred dimension;
/// see ZIPPY_PBT_NOTES.md. Runs inside the generator's `prop_map`, so it also
/// applies to every shrunk dictionary.
fn prune_unmodelable_followups(children: &mut Vec<Child>, root_chords: &[BTreeSet<char>]) {
    let siblings: Vec<BTreeSet<char>> = children.iter().map(child_chord).collect();
    let keep: Vec<bool> = children
        .iter()
        .enumerate()
        .map(|(i, c)| {
            proper_subsets(&child_chord(c)).into_iter().all(|s| {
                !root_chords.contains(&s)
                    && !siblings.iter().enumerate().any(|(j, sib)| j != i && *sib == s)
            })
        })
        .collect();
    let mut kept = keep.into_iter();
    children.retain(|_| kept.next().unwrap());
    for c in children.iter_mut() {
        prune_unmodelable_followups(&mut c.followups, root_chords);
    }
}

fn emit_children(children: &[Child], prefix: &str, lines: &mut Vec<String>) {
    for c in children {
        let keystr: String = c.keys.iter().collect();
        // One space separates components; a leading-space followup adds a second
        // (the significant space key), producing the double space the parser reads.
        let sep = if c.lead_space { "  " } else { " " };
        let input = format!("{prefix}{sep}{keystr}");
        lines.push(format!("{input}\t{}", out_to_tsv(&c.out)));
        emit_children(&c.followups, &input, lines);
    }
}

// ---------------------------------------------------------------------------
// Reference engine (the placement oracle)
// ---------------------------------------------------------------------------

fn display_len(out: &[OutItem]) -> i32 {
    out.iter()
        .map(|i| match i {
            OutItem::Char(_) => 1,
            OutItem::Backspace => -1,
        })
        .sum()
}

impl KanataModel {
    fn resolve(&self, target: &Target) -> (Vec<OutItem>, Vec<Child>, bool) {
        match target {
            Target::Root(i) => {
                let r = &self.roots[*i];
                (r.out.clone(), r.followups.clone(), false)
            }
            Target::Followup(i) => {
                let c = &self.prioritized.as_ref().unwrap()[*i];
                (c.out.clone(), c.followups.clone(), true)
            }
        }
    }

    /// First key of the target chord as written in the TSV definition: the
    /// leading space for a leading-space root, otherwise the first character
    /// (roots serialize their `BTreeSet` keys in sorted order, so the first char
    /// is the minimum — which is exactly what the parser records as the chord's
    /// first def key). Followup components are symmetric: their leading space if
    /// present, otherwise the minimum of their (sorted) keys.
    /// Mirrors `ZchChordOutput::zch_first_def_key` in the runtime.
    fn first_def_key(&self, target: &Target) -> Option<char> {
        match target {
            Target::Root(i) => self.roots.get(*i).map(|r| {
                if r.lead_space {
                    ' '
                } else {
                    *r.keys.iter().next().expect("root has >=1 key")
                }
            }),
            Target::Followup(i) => self
                .prioritized
                .as_ref()
                .and_then(|c| c.get(*i))
                .map(|c| {
                    if c.lead_space {
                        ' '
                    } else {
                        *c.keys.iter().next().expect("followup has >=1 key")
                    }
                }),
        }
    }

    fn apply_out(&mut self, out: &[OutItem]) {
        for item in out {
            match item {
                OutItem::Char(c) => self.visible.push(*c),
                OutItem::Backspace => {
                    self.visible.pop();
                }
            }
        }
    }

    /// Type one non-chord char literally: appends it and drops zippy into its
    /// post-typing disabled window. Shared by `Literal` and the tap-hold gestures
    /// (whose resolved output reaches zippy as an ordinary keystroke).
    fn type_literal(&mut self, c: char) {
        // Smart-space `full` auto-erase: a configured punctuation char typed right
        // after a full-mode activation's trailing space erases that space first
        // ("word ." -> "word."). Mirrors `zippychord.rs` (the `Sent` state + the
        // punctuation set). Only the trailing auto-space is removed; smart_space is
        // then inactive regardless.
        if self.cfg.smart_space == SmartSpace::Full
            && self.smart_space_sent
            && is_smart_space_punct(c)
            && matches!(self.visible.last(), Some(' '))
        {
            self.visible.pop();
        }
        self.smart_space_sent = false;
        self.visible.push(c);
        self.enabled = Enabled::WaitEnable;
        self.until_enabled = self.cfg.wait;
        self.prioritized = None;
        self.followup_until_clear = None;
        self.last_act_len = 0;
    }

    fn activate(
        &mut self,
        out: &[OutItem],
        followups: Vec<Child>,
        is_followup: bool,
        order_suppresses: bool,
    ) {
        if is_followup {
            // Followup replaces the prior activation's output (sitting at the tail).
            let n = self.last_act_len.min(self.visible.len());
            self.visible.truncate(self.visible.len() - n);
        }
        self.apply_out(out);
        let mut lal = display_len(out).max(0) as usize;
        // Recompute the smart-space `Sent` state from THIS activation only: the SUT
        // clears it on the activation's keystrokes and re-sets it iff this activation
        // adds a trailing space in `full` mode. Resetting here (not just in the
        // add-branch) prevents a prior `Sent` from going stale across a suppressed /
        // no-space activation and mispredicting a later punctuation auto-erase.
        self.smart_space_sent = false;
        // Smart space: add a trailing space unless output is empty or ends in
        // space/backspace — or `suppress-space` suppresses it for this press
        // order. A suppressed trailing space is identical to smart-space being
        // off: the space (and its smart_space_sent state) is simply not added.
        if self.cfg.smart_space != SmartSpace::None {
            let suppress = order_suppresses
                || out.is_empty()
                || matches!(out.last(), Some(OutItem::Backspace))
                || matches!(out.last(), Some(OutItem::Char(' ')));
            if !suppress {
                self.visible.push(' ');
                lal += 1;
                self.smart_space_sent = self.cfg.smart_space == SmartSpace::Full;
            }
        }
        self.last_act_len = lal;
        self.prioritized = if followups.is_empty() {
            None
        } else {
            Some(followups)
        };
        // Arm the followup-deadline budget exactly when a followup becomes pending
        // (dimension 12). The impl arms `ticks_until_disable = followup_deadline` on
        // the chord's final key release; the model applies the gesture atomically, so
        // the equivalent arm point is here, where `prioritized` is set.
        let fd = self.cfg.followup_deadline;
        self.followup_until_clear = self.prioritized.as_ref().map(|_| fd);
    }
}

// ---------------------------------------------------------------------------
// ReferenceStateMachine
// ---------------------------------------------------------------------------

pub struct KanataRef;

impl ReferenceStateMachine for KanataRef {
    type State = KanataModel;
    type Transition = KanataTransition;

    fn init_state() -> BoxedStrategy<Self::State> {
        (arb_cfg(), arb_roots(), arb_taphold())
            .prop_map(|(cfg, roots, taphold)| KanataModel {
                cfg,
                roots,
                taphold,
                enabled: Enabled::Enabled,
                until_enabled: 0,
                visible: vec![],
                prioritized: None,
                last_act_len: 0,
                smart_space_sent: false,
                followup_until_clear: None,
            })
            .prop_filter("must parse", |m| {
                // `Kanata::new_from_str` configures the process-global zippychord
                // state (ZCH). This filter runs during proptest's *generation*
                // phase, outside `init_test`'s guard, so it must take the same lock
                // the sim tests use — otherwise it clobbers ZCH's dictionary while
                // an unrelated sim test is mid-run, which surfaces as that test's
                // chord silently not expanding.
                let _guard = match CFG_PARSE_LOCK.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let mut fc = FxHashMap::default();
                fc.insert("file".to_string(), m.tsv());
                Kanata::new_from_str(&m.cfg_string(), fc).is_ok()
            })
            .boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        // Build chord targets reachable from the current state.
        //
        // Deferred dimension (see ZIPPY_PBT_NOTES.md): when a followup is pending
        // we offer ONLY followup targets, not fresh roots. A fresh root pressed
        // while a followup is pending can, depending on press order, trigger the
        // pending followup mid-hold (erasing the prior word) — an order-dependent
        // corner whose intended semantics are unsettled. Excluding it keeps the
        // reference's per-transition fresh/followup placement exact.
        let mut targets: Vec<(Target, Vec<char>)> = Vec::new();
        if let Some(children) = &state.prioritized {
            for (i, c) in children.iter().enumerate() {
                let mut keys: Vec<char> = c.keys.iter().copied().collect();
                if c.lead_space {
                    keys.push(' ');
                }
                targets.push((Target::Followup(i), keys));
            }
        } else {
            for (i, r) in state.roots.iter().enumerate() {
                let mut keys: Vec<char> = r.keys.iter().copied().collect();
                if r.lead_space {
                    keys.push(' ');
                }
                targets.push((Target::Root(i), keys));
            }
        }

        let chord = proptest::sample::select(targets).prop_flat_map(|(target, keys)| {
            let n = keys.len();
            let press = Just(keys.clone()).prop_shuffle();
            let release = Just(keys).prop_shuffle();
            // Per-event timing. Presses and releases each stay well within the
            // chord deadline (DEADLINE ticks) so the full chord is guaranteed to
            // form and fire; see `ChordExpansion`'s doc comment. Bounds are sized
            // for the max chord (INPUT_ALPHA + leading space) so the cumulative
            // span of either phase cannot reach DEADLINE.
            let press_delays = prop::collection::vec(0u16..=PRESS_GAP_MAX, n);
            let release_delays = prop::collection::vec(0u16..=RELEASE_GAP_MAX, n);
            (Just(target), press, release, press_delays, release_delays).prop_map(
                move |(target, press, release, press_delays, release_delays)| {
                    let mut events = Vec::with_capacity(2 * n);
                    for (k, d) in press.into_iter().zip(press_delays) {
                        events.push((d, KeyAction::Press(k)));
                    }
                    for (i, (k, d)) in release.into_iter().zip(release_delays).enumerate() {
                        // Settle for at least one tick after the final press so the
                        // activation is processed before the first release.
                        let d = if i == 0 { d.max(1) } else { d };
                        events.push((d, KeyAction::Release(k)));
                    }
                    KanataTransition::ChordExpansion { target, events }
                },
            )
        });
        // Literal types a single plain non-chord key. It must exclude tap-hold
        // input keys: pressed via Literal's brief settle the tap-hold tap stays
        // half-resolved (only TapHoldTap settles past the hold timeout), so the
        // count diverges. Tap-hold tap output is covered by TapHoldTap instead.
        let th_inputs: BTreeSet<char> = state.taphold.iter().map(|k| k.input).collect();
        let literal_alpha: Vec<char> = NONCHORD_ALPHA
            .iter()
            .chain(SMART_SPACE_PUNCT.iter())
            .copied()
            .filter(|c| !th_inputs.contains(c))
            .collect();
        let literal = proptest::sample::select(literal_alpha)
            .prop_map(|key| KanataTransition::Literal { key });
        let idle_tiny = (1u16..=3).prop_map(|ms| KanataTransition::Idle { ms });
        // Generated `wait`: a "full" idle crosses the WaitEnable countdown in one step.
        let wait = state.cfg.wait;
        let idle_full = (wait + 20..=wait + 60).prop_map(|ms| KanataTransition::Idle { ms });
        // Dimension 12: an idle that crosses the followup deadline while a followup is
        // pending. The range floor (followup_deadline + 10) is above the deadline, and
        // proptest range strategies shrink toward the *lower* bound, so this can never
        // shrink below the deadline — it always crosses, cancelling the followup into
        // the cleared-but-enabled state without ever leaking into the "fresh root
        // pressed while a followup is still pending" corner (deferred dimension 2).
        let fd = state.cfg.followup_deadline;
        let idle_cross = (fd + 10..=fd + 60).prop_map(|ms| KanataTransition::Idle { ms });
        // The PBT discovered that free typing of chord-participating keys
        // incidentally triggers chord activations, which the naive literal oracle
        // mispredicts. So free typing must exclude every key used by any chord;
        // then no combination of free-typed keys can form a chord. It must ALSO
        // exclude tap-hold input keys: a tap-hold key DELAYS its tap output, so it
        // lands out of press order relative to a plain key in the same gesture
        // (the SUT emits e.g. `wv`, the naive oracle predicts `vw`). The
        // single-key TapHoldTap/Hold transitions cover tap-hold output instead;
        // multi-key free typing over tap-hold keys is a deferred dimension. (w is
        // never a chord or tap-hold input, so the free alphabet stays non-empty.)
        let excluded = chord_keys(&state.roots);
        let free_alpha: Vec<char> = FREE_ALPHA
            .iter()
            .copied()
            .filter(|c| !excluded.contains(c) && !th_inputs.contains(c))
            .collect();
        let free = prop::collection::btree_set(proptest::sample::select(free_alpha), 1..=3)
            .prop_flat_map(|set| {
                let keys: Vec<char> = set.into_iter().collect();
                let press = Just(keys.clone()).prop_shuffle();
                let release = Just(keys).prop_shuffle();
                (press, release)
                    .prop_map(|(press, release)| KanataTransition::FreeType { press, release })
            });

        // Tap-hold gestures, offered only when the keymap has tap-hold keys.
        // Degrades to a tiny idle when there are none so the arm is always a valid
        // strategy (the weight then just adds harmless idles).
        let taphold: BoxedStrategy<KanataTransition> = if state.taphold.is_empty() {
            (1u16..=3).prop_map(|ms| KanataTransition::Idle { ms }).boxed()
        } else {
            let idxs: Vec<usize> = (0..state.taphold.len()).collect();
            let tap = proptest::sample::select(idxs.clone()).prop_map(KanataTransition::TapHoldTap);
            let hold = proptest::sample::select(idxs).prop_map(KanataTransition::TapHoldHold);
            prop_oneof![tap, hold].boxed()
        };

        // While a followup is pending the followup deadline is counting down (it was
        // armed when the preceding chord's keys were released). `idle_cross` advances
        // time past that deadline, which the model predicts exactly: the followup is
        // cancelled and zippy stays Enabled (the cleared-but-enabled state). The model
        // then offers fresh roots again, so the gesture after the expiry exercises the
        // delete-accounting/common-prefix bookkeeping in that region — the same family
        // the backspace under-count bug lived in. (Sub-deadline idles that keep the
        // followup pending are NOT generated: the residual-budget-vs-gesture-duration
        // and layout-pending-freeze interactions are a fidelity risk, and the
        // within-deadline "still fires" case is already pinned by the deterministic
        // `sim_zippychord_followup_fires_within_idle_deadline` test. See ZIPPY_PBT_NOTES.md.)
        if state.prioritized.is_some() {
            if state.taphold.is_empty() {
                prop_oneof![6 => chord, 2 => literal, 3 => free, 2 => idle_cross].boxed()
            } else {
                prop_oneof![6 => chord, 2 => literal, 3 => free, 3 => taphold, 2 => idle_cross]
                    .boxed()
            }
        } else {
            prop_oneof![
                6 => chord,
                2 => literal,
                2 => idle_tiny,
                1 => idle_full,
                3 => free,
                3 => taphold,
            ]
            .boxed()
        }
    }

    fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State {
        match transition {
            KanataTransition::Idle { ms } => {
                if let Some(budget) = state.followup_until_clear {
                    // Dimension 12: a followup is pending and its deadline is counting
                    // down (armed at the preceding chord's release). An idle that
                    // crosses the deadline cancels the followup but leaves zippy
                    // Enabled — the cleared-but-enabled state, ready for a fresh root.
                    // The committed text (and its `last_act_len`) is untouched; a
                    // subsequent fresh root appends rather than replacing. (The SM
                    // only generates crossing idles here — see `transitions` — so the
                    // `else` keep-pending branch is exercised only under shrinking.)
                    let remaining = budget.saturating_sub(*ms);
                    if remaining == 0 {
                        state.prioritized = None;
                        state.followup_until_clear = None;
                    } else {
                        state.followup_until_clear = Some(remaining);
                    }
                } else if state.enabled == Enabled::WaitEnable {
                    state.until_enabled = state.until_enabled.saturating_sub(*ms);
                    if state.until_enabled == 0 {
                        state.enabled = Enabled::Enabled;
                    }
                }
            }
            KanataTransition::Literal { key } => {
                // A non-chord key: typed literally, disables zippy (-> WaitEnable
                // on release), clears any pending followups.
                state.type_literal(*key);
            }
            KanataTransition::TapHoldTap(i) => {
                // The tap-hold key resolves to its tap output (a non-chord key),
                // which reaches zippy exactly as a literal would: types the char
                // and disables zippy. This is the layout->zippy coupling.
                let c = state.taphold[*i].tap_out;
                state.type_literal(c);
            }
            KanataTransition::TapHoldHold(i) => {
                let c = state.taphold[*i].hold_out;
                state.type_literal(c);
            }
            KanataTransition::ChordExpansion { target, events } => {
                if state.enabled == Enabled::Enabled {
                    // `not-first-def-key`: the trailing space is suppressed unless
                    // the first key pressed is the chord's first definition key.
                    let order_suppresses = match state.cfg.suppress_space {
                        SuppressSpace::None => false,
                        SuppressSpace::NotFirstDefKey => {
                            let first_pressed =
                                KanataTransition::press_order(events).first().copied();
                            let first_def = state.first_def_key(target);
                            matches!((first_pressed, first_def), (Some(p), Some(d)) if p != d)
                        }
                    };
                    let (out, followups, is_followup) = state.resolve(target);
                    state.activate(&out, followups, is_followup, order_suppresses);
                    // Activation keeps zippy enabled.
                    state.enabled = Enabled::Enabled;
                } else {
                    // Disabled passthrough: the chord does NOT fire; the keys are
                    // typed literally in press order.
                    state.smart_space_sent = false;
                    for k in KanataTransition::press_order(events) {
                        state.visible.push(k);
                    }
                    state.prioritized = None;
                    state.followup_until_clear = None;
                    state.last_act_len = 0;
                    // Release resets the wait countdown.
                    state.enabled = Enabled::WaitEnable;
                    state.until_enabled = state.cfg.wait;
                }
            }
            KanataTransition::FreeType { press, .. } => {
                // Naive oracle: predict ordinary literal typing. This is WRONG
                // whenever the typed keys form/complete a chord (the impl will
                // expand) — the PBT is meant to discover exactly that.
                state.smart_space_sent = false;
                for &k in press {
                    state.visible.push(k);
                }
                state.enabled = Enabled::WaitEnable;
                state.until_enabled = state.cfg.wait;
                state.prioritized = None;
                state.followup_until_clear = None;
                state.last_act_len = 0;
            }
        }
        state
    }

    fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool {
        // These guards keep SHRINKING inside valid space: proptest can shrink the
        // dictionary (in init_state) independently of a transition's stored keys,
        // which would otherwise produce inconsistent transitions (e.g. pressing
        // keys that no longer match the target chord, or free-typing keys that
        // became chord keys after the dict shrank) and spurious failures.
        match transition {
            KanataTransition::ChordExpansion { target, events } => {
                let target_keys: Option<BTreeSet<char>> = match target {
                    Target::Root(i) => state.roots.get(*i).map(|r| {
                        let mut k = r.keys.clone();
                        if r.lead_space {
                            k.insert(' ');
                        }
                        k
                    }),
                    Target::Followup(i) => state
                        .prioritized
                        .as_ref()
                        .and_then(|c| c.get(*i))
                        .map(|c| {
                            let mut k = c.keys.clone();
                            if c.lead_space {
                                k.insert(' ');
                            }
                            k
                        }),
                };
                match target_keys {
                    Some(tk) => {
                        // Every target key is both pressed and released exactly
                        // once, with all presses preceding all releases so the full
                        // chord is held at the last press (guaranteed activation).
                        let pressed: Vec<char> = KanataTransition::press_order(events);
                        let released: Vec<char> = events
                            .iter()
                            .filter_map(|(_, a)| match a {
                                KeyAction::Release(c) => Some(*c),
                                KeyAction::Press(_) => None,
                            })
                            .collect();
                        let last_press = events
                            .iter()
                            .rposition(|(_, a)| matches!(a, KeyAction::Press(_)));
                        let first_release = events
                            .iter()
                            .position(|(_, a)| matches!(a, KeyAction::Release(_)));
                        let ordered = match (last_press, first_release) {
                            (Some(lp), Some(fr)) => lp < fr,
                            _ => true,
                        };
                        ordered
                            && pressed.iter().copied().collect::<BTreeSet<_>>() == tk
                            && released.iter().copied().collect::<BTreeSet<_>>() == tk
                    }
                    None => false,
                }
            }
            KanataTransition::FreeType { press, release } => {
                let chords = chord_keys(&state.roots);
                let th: BTreeSet<char> = state.taphold.iter().map(|k| k.input).collect();
                let ok = |k: &char| !chords.contains(k) && !th.contains(k);
                press.iter().all(ok) && release.iter().all(ok)
            }
            // Guard against the tap-hold list shrinking out from under a stored
            // index (init_state shrinks independently of transitions).
            KanataTransition::TapHoldTap(i) | KanataTransition::TapHoldHold(i) => {
                *i < state.taphold.len()
            }
            // A literal must stay a plain key; if the dict shrank a key into a
            // tap-hold input, drop it (tap-hold tap is covered by TapHoldTap).
            KanataTransition::Literal { key } => {
                !state.taphold.iter().any(|k| k.input == *key)
            }
            // Dimension 12: while a followup is pending the only idle the model
            // handles exactly is one that *crosses* the deadline (cancel → cleared-
            // but-enabled). A sub-deadline idle would leave the followup pending, and
            // a later root-target transition would then apply while pending — the
            // unmodeled "fresh root while a followup is pending" corner (dimension 2).
            // The generator only emits crossing idles here; this guard keeps that true
            // under shrinking. (`followup_until_clear == DEADLINE` whenever pending.)
            KanataTransition::Idle { ms } => match state.followup_until_clear {
                Some(budget) => *ms >= budget,
                None => true,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn arb_cfg() -> impl Strategy<Value = ModelCfg> {
    prop_oneof![
        Just(SmartSpace::None),
        Just(SmartSpace::AddOnly),
        Just(SmartSpace::Full),
    ]
    .prop_flat_map(|smart_space| {
        // suppress-space only has an effect with an active smart-space; pairing
        // it with `none` would just be inert (and warn at parse), so concentrate
        // coverage where the trailing space actually exists.
        let suppress: BoxedStrategy<SuppressSpace> = if smart_space == SmartSpace::None {
            Just(SuppressSpace::None).boxed()
        } else {
            prop_oneof![Just(SuppressSpace::None), Just(SuppressSpace::NotFirstDefKey)].boxed()
        };
        // Timers, generated independently within safe bounds:
        //  - chord_deadline ≥ 50: the proven floor for the ≤30-tick gesture span
        //    (PRESS_GAP_MAX was sized against 50), so every chord still forms.
        //  - followup_deadline generated *separately* (and allowed larger) so the
        //    followup-vs-initial divergence is exercised, not assumed equal.
        //  - wait > 250: above the tap-hold settle (TH_HOLD_TIMEOUT + 50), so a tap
        //    cannot re-enable zippy mid-gesture and desync the coarse oracle.
        let timers = (50u16..=100, 50u16..=150, 300u16..=600);
        (suppress, timers).prop_map(
            move |(suppress_space, (chord_deadline, followup_deadline, wait))| ModelCfg {
                smart_space,
                suppress_space,
                chord_deadline,
                followup_deadline,
                wait,
            },
        )
    })
}

fn arb_out() -> impl Strategy<Value = Vec<OutItem>> {
    // Deferred dimension (ZIPPY_PBT_NOTES.md): `⌫` (backspace) in output — the
    // suffix-chord pattern — interacts with already-committed text and makes the
    // "tail length this activation owns" ambiguous; not modeled yet.
    let item = prop::sample::select(&['a', 'b', 'c', 'A', 'B', ' '][..]).prop_map(OutItem::Char);
    prop::collection::vec(item, 1..=4)
}

fn arb_child(depth: u32) -> BoxedStrategy<Child> {
    let followups = if depth == 0 {
        Just(Vec::new()).boxed()
    } else {
        prop::collection::vec(arb_child(depth - 1), 0..=1).boxed()
    };
    let keys =
        prop::collection::btree_set(prop::sample::select(INPUT_ALPHA), 1..=INPUT_ALPHA.len());
    (any::<bool>(), keys, arb_out(), followups)
        .prop_map(|(lead_space, keys, out, followups)| Child {
            lead_space,
            keys,
            out,
            // dedup sibling children by their full chord (lead space + key set)
            followups: dedup_children(followups),
        })
        .boxed()
}

fn dedup_children(children: Vec<Child>) -> Vec<Child> {
    let mut seen = BTreeSet::new();
    children
        .into_iter()
        .filter(|c| seen.insert((c.lead_space, c.keys.clone())))
        .collect()
}

/// 0..=2 tap-hold keys for the layout. Inputs are drawn from NONCHORD_ALPHA
/// (disjoint from chord keys), tap output = the key itself (an ordinary
/// keystroke), hold output = a distinct non-chord key. Empty is allowed (and is
/// the default for older seeds) — then the model reduces to the pure zippy SM.
fn arb_taphold() -> impl Strategy<Value = Vec<ThKey>> {
    // inputs/tap from the low half (u,v), hold from the high half (x,y) so a hold
    // output is never itself a tap-hold input.
    (0usize..=2).prop_map(|n| {
        (0..n)
            .map(|i| ThKey {
                input: NONCHORD_ALPHA[i],
                tap_out: NONCHORD_ALPHA[i],
                hold_out: NONCHORD_ALPHA[i + 3],
            })
            .collect()
    })
}

fn arb_root() -> impl Strategy<Value = Root> {
    (
        any::<bool>(),
        prop::collection::btree_set(prop::sample::select(INPUT_ALPHA), 1..=INPUT_ALPHA.len()),
        arb_out(),
        prop::collection::vec(arb_child(1), 0..=2),
    )
        .prop_map(|(lead_space, keys, out, followups)| Root {
            lead_space,
            keys,
            out,
            followups: dedup_children(followups),
        })
}

fn arb_roots() -> impl Strategy<Value = Vec<Root>> {
    prop::collection::vec(arb_root(), 1..=5).prop_map(|roots| {
        let mut seen = BTreeSet::new();
        let mut roots: Vec<Root> = roots
            .into_iter()
            .filter(|r| seen.insert(root_chord(r)))
            .collect();
        // Prune followups whose multi-key formation would eagerly activate a
        // sibling or root mid-press (unmodelable by the placement oracle). Needs
        // the full root key-set list, so it runs after roots are deduped.
        let root_chords: Vec<BTreeSet<char>> = roots.iter().map(root_chord).collect();
        for r in roots.iter_mut() {
            prune_unmodelable_followups(&mut r.followups, &root_chords);
        }
        roots
    })
}

// ---------------------------------------------------------------------------
// SUT
// ---------------------------------------------------------------------------

pub struct Sut {
    kanata: Kanata,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for Sut {
    fn drop(&mut self) {
        // Clear the global PRESSED_KEYS so a panic mid-scenario (the assertion
        // failure that drives shrinking) cannot leak held keys into later tests.
        crate::PRESSED_KEYS.lock().clear();
    }
}

fn osc_of(c: char) -> crate::OsCode {
    let tok = if c == ' ' {
        "spc".to_string()
    } else {
        c.to_string()
    };
    str_to_oscode(&tok).expect("valid key")
}

fn pressed_insert(_osc: crate::OsCode) {
    #[cfg(not(all(target_os = "windows", not(feature = "interception_driver"))))]
    crate::PRESSED_KEYS.lock().insert(_osc);
    #[cfg(all(target_os = "windows", not(feature = "interception_driver")))]
    crate::PRESSED_KEYS
        .lock()
        .insert(_osc, web_time::Instant::now());
}

fn pressed_remove(osc: crate::OsCode) {
    crate::PRESSED_KEYS.lock().remove(&osc);
}

fn feed_press(k: &mut Kanata, c: char) {
    let o = osc_of(c);
    k.handle_input_event(&KeyEvent::new(o, KeyValue::Press))
        .unwrap();
    pressed_insert(o);
    k.tick_ms(1, &None).unwrap();
}

fn feed_release(k: &mut Kanata, c: char) {
    let o = osc_of(c);
    k.handle_input_event(&KeyEvent::new(o, KeyValue::Release))
        .unwrap();
    pressed_remove(o);
    k.tick_ms(1, &None).unwrap();
}

/// Output-stream key-state invariant: a key must never be pressed (`out:↓`)
/// twice without an intervening release (`out:↑`). A second down of an
/// already-held key relies on the OS coalescing the two into one held key, which
/// silently drops the second press's effect — this is exactly how a leading-space
/// chord activated space-first loses its smart-space trailing space (the eager
/// participating `Space` is never released before smart-space presses `Space`
/// again). Returns `Err` naming the offending key on the first violation.
///
/// Key *repeat* is a distinct event (not a second `Press`), so it is not a
/// counterexample; the PBT never generates repeats.
pub(super) fn check_no_double_press(events: &str) -> Result<(), String> {
    let mut down: BTreeSet<&str> = BTreeSet::new();
    for tok in events.split_whitespace() {
        if let Some(name) = tok.strip_prefix("out:↓") {
            if !down.insert(name) {
                return Err(format!("key {name} pressed twice without a release"));
            }
        } else if let Some(name) = tok.strip_prefix("out:↑") {
            down.remove(name);
        }
    }
    Ok(())
}

// ===========================================================================
// Capability-selected invariant catalog (shared spine)
// ===========================================================================
//
// Decomposition discipline (see ZIPPY_PBT_NOTES.md): kanata's features are
// modelled as the real runtime *components* (capabilities), NOT one cap per
// feature and never a cap named after a property. A test slice declares which
// capabilities its generated keymap exercises; the catalog runs exactly the
// invariants whose need-set is satisfied. `OutputKeyState` is universal, so the
// legality invariants run over every slice for free (coverage is multiplicative:
// a new invariant lights up every slice that has its caps).
//
// Selection rule: runs ⟺ needs_pos ⊆ present ∧ needs_neg ∩ present = ∅. The
// negative half is for degraded-mode twins (e.g. smart-space full vs none).

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Cap {
    /// The serialized output keystroke stream (which keys are down). Universal.
    OutputKeyState,
    /// The reconstructed visible-text buffer (zippychord + smart-space write it).
    VisibleText,
    /// Zippychord engine state (enabled/disabled, pending followups).
    ZippyState,
    /// Layout/layer resolution state (tap-hold, layers, oneshot, tap-dance, ...).
    LayoutState,
}

pub(super) type CapSet = BTreeSet<Cap>;

pub(super) fn capset(caps: &[Cap]) -> CapSet {
    caps.iter().copied().collect()
}

/// What an invariant sees after a transition: the raw cumulative output event
/// stream, plus whether the SUT is *quiescent* (all physical input keys released
/// and output flushed). Some invariants (e.g. clean-release) only hold at rest.
pub(super) struct InvCtx<'a> {
    pub events: &'a str,
    pub quiescent: bool,
    /// Just the output events emitted by the *current* transition (the cumulative
    /// `events` minus everything before this step). Event-stream invariants that
    /// reason about a single activation's keystrokes (e.g. eager-undo efficiency)
    /// use this; whole-stream legality invariants use `events`.
    pub step_events: &'a str,
    /// The reconstructed visible text *before* this transition — the buffer the
    /// `step_events` replay starts from.
    pub prev_text: &'a str,
}

/// One catalog invariant, tagged with the component capabilities it requires
/// present (`needs_pos`) and absent (`needs_neg`). `id` is stable so
/// selection-parity checks can name it.
struct Invariant {
    id: &'static str,
    needs_pos: &'static [Cap],
    needs_neg: &'static [Cap],
    check: fn(&InvCtx) -> Result<(), String>,
}

impl Invariant {
    fn selected(&self, present: &CapSet) -> bool {
        self.needs_pos.iter().all(|c| present.contains(c))
            && self.needs_neg.iter().all(|c| !present.contains(c))
    }
}

fn inv_no_double_press(ctx: &InvCtx) -> Result<(), String> {
    check_no_double_press(ctx.events)
}

// NOTE: a tempting "no release without a matching press" invariant is NOT sound
// for kanata. Zippychord's capitalize idiom re-asserts a key as `↑X ↓X` while
// shift is held (see `sim_zippychord_capitalize`), and when the eager `↓X` was
// not emitted it issues a *phantom* `↑X` — harmless on a real OS (releasing an
// unheld key is a no-op). The catalog process discovered this by adding the
// invariant and watching it fail on intended behaviour; it was dropped rather
// than weakened to model the idiom. (Deferred: a refined version that tolerates
// the `↑X ↓X` re-assert could be added later.)

/// At rest (quiescent), no output key may remain held: every `out:↓` must have a
/// matching `out:↑`. Catches dangling held keys — e.g. the leading-space eager
/// `Space↓` that is never released. Subsumes modifier-balance at rest. Only runs
/// at quiescence (a key held mid-gesture is legitimate).
fn inv_clean_release(ctx: &InvCtx) -> Result<(), String> {
    if !ctx.quiescent {
        return Ok(());
    }
    let mut down: BTreeSet<&str> = BTreeSet::new();
    for tok in ctx.events.split_whitespace() {
        if let Some(name) = tok.strip_prefix("out:↓") {
            down.insert(name);
        } else if let Some(name) = tok.strip_prefix("out:↑") {
            down.remove(name);
        }
    }
    if down.is_empty() {
        Ok(())
    } else {
        let held: Vec<&str> = down.into_iter().collect();
        Err(format!("keys still held at rest: {}", held.join(", ")))
    }
}

/// Replay one transition's output events on the prior visible buffer, yielding the
/// ordered (insert char / delete) operations and the running buffer. Shared by the
/// text-efficiency/legality invariants. Modifiers set the shift state for casing;
/// non-printing keys are ignored. Returns the per-position "most recently deleted
/// value" trace alongside the final redundancy/void findings.
fn common_prefix_len(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Replay one activation's output ops and measure two things:
/// - `redundant`: backspaces that deleted *below the common prefix* the impl could
///   have kept. A backspace-only replacement of old→new must delete down to
///   `common_prefix_len(old, new)` and no further; reaching a lower floor and
///   retyping those shared-prefix chars is pure waste. Crucially this is NOT "any
///   deleted char that reappears" — a char beyond the common prefix (e.g. a
///   coincidental match at the tail of a fully-different word) MUST be deleted to
///   reach the divergence, so retyping it is necessary, not redundant.
/// - `void_deletes`: backspaces on an empty buffer (deleting past the prompt).
///
/// "old" for an episode is the buffer at its peak (just before the burst's first
/// backspace); "new" is the final buffer. `floor` is the lowest length the burst
/// reached. Redundancy for the episode is `max(0, common_prefix(old,new) - floor)`.
fn replay_step(prev_text: &str, step_events: &str) -> (usize, usize) {
    let mut buf: Vec<char> = prev_text.chars().collect();
    let mut shift = false;
    let mut void_deletes = 0usize;
    // Per delete-burst: (peak buffer before the burst, lowest length reached).
    let mut episodes: Vec<(Vec<char>, usize)> = Vec::new();
    let mut in_delete = false;
    let mut peak: Vec<char> = Vec::new();
    let mut floor = 0usize;
    for tok in step_events.split_whitespace() {
        let typed: Option<char> = if let Some(name) = tok.strip_prefix("out:↓") {
            match name {
                "LShift" | "RShift" => {
                    shift = true;
                    None
                }
                "BSpace" => {
                    if !in_delete {
                        in_delete = true;
                        peak = buf.clone();
                        floor = buf.len();
                    }
                    match buf.pop() {
                        Some(_) => floor = floor.min(buf.len()),
                        None => void_deletes += 1,
                    }
                    None
                }
                "Space" => Some(' '),
                n => key_to_char(n).map(|c| if shift { c.to_ascii_uppercase() } else { c }),
            }
        } else if let Some(name) = tok.strip_prefix("out:↑") {
            if matches!(name, "LShift" | "RShift") {
                shift = false;
            }
            None
        } else {
            None
        };
        if let Some(ch) = typed {
            if in_delete {
                episodes.push((std::mem::take(&mut peak), floor));
                in_delete = false;
            }
            buf.push(ch);
        }
    }
    if in_delete {
        episodes.push((peak, floor));
    }
    let redundant = episodes
        .iter()
        .map(|(old, floor)| common_prefix_len(old, &buf).saturating_sub(*floor))
        .sum();
    (redundant, void_deletes)
}

/// Number of backspaces that delete *below the common prefix* the impl could have
/// preserved across an eager/echo/followup replacement. >0 means wasted deletes
/// (correct net text, but fragile when deletes drop under load). Exposed for the
/// `sim_zippychord_redundant_echo_delete` repro and the detector unit tests.
pub(super) fn redundant_prefix_deletes(prev_text: &str, step_events: &str) -> usize {
    replay_step(prev_text, step_events).0
}

/// Efficiency: within a single activation, no backspace deletes below the common
/// prefix shared between the transiently-shown text (eager expansion or echoed input
/// keys) and the final text. Deleting a shared-prefix char and retyping it is pure
/// waste — and the source of corruption when those deletes drop under load / on a
/// laggy remote. The common-prefix optimization in `zippychord.rs` already preserves
/// prefixes *between expansions*; this is its teeth, and currently RED for the echoed-
/// input case (see `sim_zippychord_redundant_echo_delete`). It is a *live* catalog
/// invariant: to run the correctness-only suite green, disable it via
/// `KANATA_PBT_DISABLE_INVARIANTS=no_redundant_prefix_delete`. (Distinct from the
/// rejected `no_release_without_press`: the capitalize idiom re-asserts via `↑X ↓X`,
/// never via a backspace, so it is not flagged here.)
fn inv_no_redundant_prefix_delete(ctx: &InvCtx) -> Result<(), String> {
    let redundant = redundant_prefix_deletes(ctx.prev_text, ctx.step_events);
    if redundant == 0 {
        Ok(())
    } else {
        Err(format!(
            "{redundant} char(s) backspaced then retyped identically (common prefix not \
             preserved across an eager/echo/followup replacement)"
        ))
    }
}

/// Legality: no backspace deletes past the start of the buffer (into the user's
/// pre-existing text / the prompt). Backspacing into earlier committed words is
/// legitimate (a followup replaces the prior word); deleting into the void is not.
fn inv_no_delete_into_void(ctx: &InvCtx) -> Result<(), String> {
    let (_, void_deletes) = replay_step(ctx.prev_text, ctx.step_events);
    if void_deletes == 0 {
        Ok(())
    } else {
        Err(format!(
            "{void_deletes} backspace(s) deleted past the start of the buffer"
        ))
    }
}

/// The single shared catalog. Authored once; every slice runs the selected
/// subset over the same tick. Add an entry here and it lights up every slice
/// that has its capabilities — no per-slice duplication.
static INVARIANTS: &[Invariant] = &[
    Invariant {
        id: "no_double_press",
        needs_pos: &[Cap::OutputKeyState],
        needs_neg: &[],
        check: inv_no_double_press,
    },
    Invariant {
        id: "clean_release",
        needs_pos: &[Cap::OutputKeyState],
        needs_neg: &[],
        check: inv_clean_release,
    },
    // OPEN finding: fires on a large fraction of chords (any whose output extends its
    // echoed input keys). Live so the PBT surfaces *every* instance; disable with
    // `KANATA_PBT_DISABLE_INVARIANTS=no_redundant_prefix_delete` to run the
    // correctness-only suite green. See ZIPPY_PBT_NOTES.md.
    Invariant {
        id: "no_redundant_prefix_delete",
        needs_pos: &[Cap::OutputKeyState, Cap::VisibleText],
        needs_neg: &[],
        check: inv_no_redundant_prefix_delete,
    },
    Invariant {
        id: "no_delete_into_void",
        needs_pos: &[Cap::OutputKeyState, Cap::VisibleText],
        needs_neg: &[],
        check: inv_no_delete_into_void,
    },
];

/// Invariant ids switched off via `KANATA_PBT_DISABLE_INVARIANTS` (comma-separated).
/// Lets a run opt out of an invariant without editing the catalog — e.g. disable the
/// OPEN `no_redundant_prefix_delete` efficiency check to run the correctness-only
/// suite green: `KANATA_PBT_DISABLE_INVARIANTS=no_redundant_prefix_delete cargo test`.
fn parse_disabled_list(s: &str) -> BTreeSet<String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}

fn disabled_invariant_ids() -> BTreeSet<String> {
    std::env::var("KANATA_PBT_DISABLE_INVARIANTS")
        .ok()
        .map(|s| parse_disabled_list(&s))
        .unwrap_or_default()
}

/// Run every selected, non-disabled invariant over one transition's outcome; returns
/// the offending invariant id + message on the first violation.
pub(super) fn run_invariants(present: &CapSet, ctx: &InvCtx) -> Result<(), (&'static str, String)> {
    run_invariants_filtered(present, ctx, &disabled_invariant_ids())
}

fn run_invariants_filtered(
    present: &CapSet,
    ctx: &InvCtx,
    disabled: &BTreeSet<String>,
) -> Result<(), (&'static str, String)> {
    for inv in INVARIANTS {
        if inv.selected(present) && !disabled.contains(inv.id) {
            (inv.check)(ctx).map_err(|e| (inv.id, e))?;
        }
    }
    Ok(())
}

/// Ids of the invariants selected for a slice. Used by the selection self-tests
/// to assert a non-empty, expected footprint — a slice can otherwise be green
/// purely because every property touching its weak spot was deselected.
pub(super) fn selected_ids(present: &CapSet) -> Vec<&'static str> {
    INVARIANTS
        .iter()
        .filter(|i| i.selected(present))
        .map(|i| i.id)
        .collect()
}

/// Reconstruct net visible text from raw `out:↓X`/`out:↑X` events.
pub(super) fn net_text(events: &str) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut shift = false;
    for tok in events.split_whitespace() {
        if let Some(name) = tok.strip_prefix("out:↓") {
            match name {
                "LShift" | "RShift" => shift = true,
                "BSpace" => {
                    out.pop();
                }
                "Space" => out.push(' '),
                n => {
                    if let Some(c) = key_to_char(n) {
                        out.push(if shift { c.to_ascii_uppercase() } else { c });
                    }
                }
            }
        } else if let Some(name) = tok.strip_prefix("out:↑") {
            if matches!(name, "LShift" | "RShift") {
                shift = false;
            }
        }
    }
    out.into_iter().collect()
}

fn key_to_char(name: &str) -> Option<char> {
    // Generated smart-space punctuation (default set is `.`/`,`/`;`; only `.` is
    // generated, the only one whose output key-name we pin here).
    if name == "Dot" {
        return Some('.');
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

pub(super) fn sut_net_text(k: &Kanata) -> String {
    let events = k.kbd_out.outputs.events.join(" ");
    net_text(&events)
}

impl StateMachineTest for Sut {
    type SystemUnderTest = Sut;
    type Reference = KanataRef;

    fn init_test(ref_state: &KanataModel) -> Self::SystemUnderTest {
        let guard = match CFG_PARSE_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        crate::PRESSED_KEYS.lock().clear();
        let mut fc = FxHashMap::default();
        fc.insert("file".to_string(), ref_state.tsv());
        let kanata =
            Kanata::new_from_str(&ref_state.cfg_string(), fc).expect("generated cfg must parse");
        Sut {
            kanata,
            _guard: guard,
        }
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        ref_state: &KanataModel,
        transition: KanataTransition,
    ) -> Self::SystemUnderTest {
        let k = &mut state.kanata;
        // Output events emitted before this transition — used to slice out just
        // this step's keystrokes (the per-activation efficiency invariants need the
        // single-activation delta, not the cumulative stream).
        let before_len = k.kbd_out.outputs.events.len();
        match &transition {
            KanataTransition::Idle { ms } => {
                k.tick_ms(*ms as u128, &None).unwrap();
            }
            KanataTransition::Literal { key } => {
                feed_press(k, *key);
                feed_release(k, *key);
            }
            KanataTransition::TapHoldTap(i) => {
                // Press + release before the hold timeout => tap output. The
                // settle tick stays below WAIT so it can't re-enable mid-transition
                // (the reference doesn't decrement until_enabled here); the bimodal
                // idle generator guarantees no idle lands in the ambiguous middle.
                let c = ref_state.taphold[*i].input;
                feed_press(k, c);
                k.tick_ms(4, &None).unwrap();
                feed_release(k, c);
                k.tick_ms((TH_HOLD_TIMEOUT + 50) as u128, &None).unwrap();
            }
            KanataTransition::TapHoldHold(i) => {
                // Hold past the hold timeout => hold output, then release.
                let c = ref_state.taphold[*i].input;
                feed_press(k, c);
                k.tick_ms((TH_HOLD_TIMEOUT + 5) as u128, &None).unwrap();
                feed_release(k, c);
                k.tick_ms(50, &None).unwrap();
            }
            KanataTransition::ChordExpansion { events, .. } => {
                for (delay, action) in events {
                    if *delay > 0 {
                        k.tick_ms(*delay as u128, &None).unwrap();
                    }
                    match action {
                        KeyAction::Press(c) => feed_press(k, *c),
                        KeyAction::Release(c) => feed_release(k, *c),
                    }
                }
            }
            KanataTransition::FreeType { press, release } => {
                for &c in press {
                    feed_press(k, c);
                }
                k.tick_ms(1, &None).unwrap();
                for &c in release {
                    feed_release(k, c);
                }
            }
        }
        let all = &k.kbd_out.outputs.events;
        let raw = all.join(" ");
        // This transition's own output delta, and the visible text just before it —
        // the starting buffer for the per-activation efficiency/legality replay.
        let step_events = all[before_len..].join(" ");
        let prev_text = net_text(&all[..before_len].join(" "));
        // Capability-selected catalog invariants (independent of the net-text
        // oracle, which is blind to OS key coalescing). This slice's components:
        // the output stream, the visible-text buffer, the zippy engine, and the
        // layout/layer resolution (tap-hold keys).
        let present = capset(&[
            Cap::OutputKeyState,
            Cap::VisibleText,
            Cap::ZippyState,
            Cap::LayoutState,
        ]);
        // Every generated transition presses then releases all its keys, so the
        // SUT is quiescent afterwards (no physical key held) — clean-release then
        // requires the output stream to be balanced too.
        let quiescent = crate::PRESSED_KEYS.lock().is_empty();
        let ctx = InvCtx {
            events: &raw,
            quiescent,
            step_events: &step_events,
            prev_text: &prev_text,
        };
        if let Err((id, e)) = run_invariants(&present, &ctx) {
            panic!(
                "invariant `{id}` violated: {e}\n  transition: {transition:?}\n  cfg: {}\n  dict: {}\n  raw: {raw}",
                ref_state.cfg_string(),
                ref_state.tsv().replace('\n', " | "),
            );
        }
        let got = sut_net_text(k);
        let expected: String = ref_state.visible.iter().collect();
        assert_eq!(
            expected,
            got,
            "\n  transition: {:?}\n  cfg: {}\n  dict: {}\n  raw: {}",
            transition,
            ref_state.cfg_string(),
            ref_state.tsv().replace('\n', " | "),
            raw
        );
        state
    }
}

prop_state_machine_persisted! {
    #![proptest_config(Config { cases: 3000, .. Config::default() })]
    #[test]
    fn zippychord_state_machine(sequential 1..32 => Sut);
}

// ---------------------------------------------------------------------------
// Reference self-consistency tests: drive KanataRef::apply directly and check the
// predicted `visible` against hand-computed expectations. These validate that
// the oracle is trustworthy (so a state-machine failure means a real impl bug,
// not a reference bug).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod reference_tests {
    use super::*;

    fn out(s: &str) -> Vec<OutItem> {
        s.chars().map(OutItem::Char).collect()
    }
    fn root(lead_space: bool, keys: &str, o: &str, followups: Vec<Child>) -> Root {
        Root {
            lead_space,
            keys: keys.chars().collect(),
            out: out(o),
            followups,
        }
    }
    fn child(lead_space: bool, keys: &str, o: &str, followups: Vec<Child>) -> Child {
        Child {
            lead_space,
            keys: keys.chars().collect(),
            out: out(o),
            followups,
        }
    }
    fn model(smart_space: SmartSpace, roots: Vec<Root>) -> KanataModel {
        model_suppress(smart_space, SuppressSpace::None, roots)
    }
    fn model_suppress(
        smart_space: SmartSpace,
        suppress_space: SuppressSpace,
        roots: Vec<Root>,
    ) -> KanataModel {
        KanataModel {
            cfg: ModelCfg {
                smart_space,
                suppress_space,
                chord_deadline: DEADLINE,
                followup_deadline: DEADLINE,
                wait: WAIT,
            },
            roots,
            taphold: vec![],
            enabled: Enabled::Enabled,
            until_enabled: 0,
            visible: vec![],
            prioritized: None,
            last_act_len: 0,
            smart_space_sent: false,
            followup_until_clear: None,
        }
    }
    fn chord(target: Target, keys: &str) -> KanataTransition {
        // All presses (delay 0) then all releases — the smart generator's
        // invariant — so the reference's disabled-passthrough press order is well
        // defined. The enabled path ignores the events entirely.
        let mut events: Vec<(u16, KeyAction)> =
            keys.chars().map(|c| (0u16, KeyAction::Press(c))).collect();
        events.extend(keys.chars().map(|c| (0u16, KeyAction::Release(c))));
        KanataTransition::ChordExpansion { target, events }
    }
    fn apply(m: KanataModel, tr: &KanataTransition) -> KanataModel {
        <KanataRef as ReferenceStateMachine>::apply(m, tr)
    }
    fn vis(m: &KanataModel) -> String {
        m.visible.iter().collect()
    }

    #[test]
    fn ref_single_fresh() {
        let m = model(SmartSpace::None, vec![root(false, "ab", "xy", vec![])]);
        let m = apply(m, &chord(Target::Root(0), "ab"));
        assert_eq!("xy", vis(&m));
    }

    #[test]
    fn ref_two_words_append() {
        let m = model(
            SmartSpace::None,
            vec![root(false, "a", "P", vec![]), root(false, "b", "Q", vec![])],
        );
        let m = apply(m, &chord(Target::Root(0), "a"));
        let m = apply(m, &chord(Target::Root(1), "b"));
        assert_eq!("PQ", vis(&m));
    }

    #[test]
    fn ref_target_is_final_chord_output() {
        // Pressing the larger chord's keys yields its output regardless of any
        // smaller subset chord (the reference places the target output).
        let m = model(
            SmartSpace::None,
            vec![
                root(false, "a", "P", vec![]),
                root(false, "ab", "QQ", vec![]),
            ],
        );
        let m = apply(m, &chord(Target::Root(1), "ab"));
        assert_eq!("QQ", vis(&m));
    }

    #[test]
    fn ref_leading_space_swallowed() {
        // " a" -> "a": the participating space is not part of the output.
        let m = model(SmartSpace::None, vec![root(true, "a", "a", vec![])]);
        let m = apply(m, &chord(Target::Root(0), "a "));
        assert_eq!("a", vis(&m));
    }

    #[test]
    fn ref_followup_replaces_prior() {
        let m = model(
            SmartSpace::None,
            vec![root(false, "a", "X", vec![child(false, "b", "Y", vec![])])],
        );
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("X", vis(&m));
        let m = apply(m, &chord(Target::Followup(0), "b"));
        assert_eq!("Y", vis(&m));
    }

    #[test]
    fn ref_followup_deadline_expiry_clears_but_stays_enabled() {
        // Dimension 12: a root with a pending followup, then an idle that crosses the
        // followup deadline. The model cancels the followup, keeps the committed text,
        // and stays Enabled — so a subsequent *fresh* root APPENDS (does not replace).
        let m = model(
            SmartSpace::None,
            vec![
                root(false, "a", "X", vec![child(false, "b", "Y", vec![])]),
                root(false, "c", "Z", vec![]),
            ],
        );
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("X", vis(&m));
        assert!(m.prioritized.is_some());
        assert_eq!(Some(DEADLINE), m.followup_until_clear);

        // Idle past the deadline: followup cancelled, zippy still Enabled.
        let m = apply(m, &KanataTransition::Idle { ms: DEADLINE + 10 });
        assert_eq!("X", vis(&m), "committed text is untouched by expiry");
        assert!(m.prioritized.is_none(), "followup cancelled");
        assert_eq!(None, m.followup_until_clear);
        assert_eq!(Enabled::Enabled, m.enabled, "stays enabled, not disabled");

        // A fresh root now appends rather than replacing the prior word.
        let m = apply(m, &chord(Target::Root(1), "c"));
        assert_eq!("XZ", vis(&m));
    }

    #[test]
    fn ref_followup_still_fires_when_no_idle_intervenes() {
        // Control for the test above: with no crossing idle, the followup replaces.
        let m = model(
            SmartSpace::None,
            vec![root(false, "a", "X", vec![child(false, "b", "Y", vec![])])],
        );
        let m = apply(m, &chord(Target::Root(0), "a"));
        let m = apply(m, &chord(Target::Followup(0), "b"));
        assert_eq!("Y", vis(&m));
    }

    #[test]
    fn ref_smart_space_full_punct_erases_trailing_space() {
        // smart-space full: expansion "X" -> "X " (trailing space, Sent). A following
        // punctuation literal '.' erases the space -> "X.". A non-punct literal does
        // not (control).
        let m = model(SmartSpace::Full, vec![root(false, "a", "X", vec![])]);
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("X ", vis(&m));
        assert!(m.smart_space_sent);
        let m = apply(m, &KanataTransition::Literal { key: '.' });
        assert_eq!("X.", vis(&m), "punctuation must erase the auto-added space");

        // Control: a non-punct literal keeps the space.
        let m2 = model(SmartSpace::Full, vec![root(false, "a", "X", vec![])]);
        let m2 = apply(m2, &chord(Target::Root(0), "a"));
        let m2 = apply(m2, &KanataTransition::Literal { key: 'u' });
        assert_eq!("X u", vis(&m2), "a non-punct literal must keep the space");
    }

    #[test]
    fn ref_smart_space_add_only_appends_space() {
        let m = model(SmartSpace::AddOnly, vec![root(false, "a", "X", vec![])]);
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("X ", vis(&m));
    }

    #[test]
    fn ref_smart_space_followup_replaces_with_trailing_space() {
        let m = model(
            SmartSpace::AddOnly,
            vec![root(false, "a", "day", vec![child(false, "b", "Monday", vec![])])],
        );
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("day ", vis(&m));
        let m = apply(m, &chord(Target::Followup(0), "b"));
        assert_eq!("Monday ", vis(&m));
    }

    #[test]
    fn ref_suppress_not_first_def_key() {
        // Chord {a,b}; first def key is the min, 'a'.
        // Press 'a' first (== first def) -> keep the trailing space.
        let m = model_suppress(
            SmartSpace::AddOnly,
            SuppressSpace::NotFirstDefKey,
            vec![root(false, "ab", "X", vec![])],
        );
        let kept = apply(m, &chord(Target::Root(0), "ab"));
        assert_eq!("X ", vis(&kept));

        // Press 'b' first (!= first def) -> suppress the trailing space.
        let m = model_suppress(
            SmartSpace::AddOnly,
            SuppressSpace::NotFirstDefKey,
            vec![root(false, "ab", "X", vec![])],
        );
        let suppressed = apply(m, &chord(Target::Root(0), "ba"));
        assert_eq!("X", vis(&suppressed));
    }

    #[test]
    fn ref_suppress_not_first_def_key_leading_space() {
        // Leading-space chord " a" -> "a"; first def key is SPACE.
        // SPACE first (== first def) -> keep trailing space -> "a ".
        let m = model_suppress(
            SmartSpace::Full,
            SuppressSpace::NotFirstDefKey,
            vec![root(true, "a", "a", vec![])],
        );
        let kept = apply(m, &chord(Target::Root(0), " a"));
        assert_eq!("a ", vis(&kept));

        // 'a' first (!= first def SPACE) -> suppress -> "a".
        let m = model_suppress(
            SmartSpace::Full,
            SuppressSpace::NotFirstDefKey,
            vec![root(true, "a", "a", vec![])],
        );
        let suppressed = apply(m, &chord(Target::Root(0), "a "));
        assert_eq!("a", vis(&suppressed));
    }

    #[test]
    fn ref_suppress_followup_single_key_never_suppressed() {
        // A single-key followup has only one possible first pressed key, which is
        // therefore always its first def key -> it keeps its trailing space.
        let m = model_suppress(
            SmartSpace::AddOnly,
            SuppressSpace::NotFirstDefKey,
            vec![root(false, "ab", "day", vec![child(false, "c", "Monday", vec![])])],
        );
        // Root pressed 'a' first (== def) -> "day ".
        let m = apply(m, &chord(Target::Root(0), "ab"));
        assert_eq!("day ", vis(&m));
        let m = apply(m, &chord(Target::Followup(0), "c"));
        assert_eq!("Monday ", vis(&m));
    }

    #[test]
    fn ref_multi_key_followup_replaces_and_smart_spaces() {
        // `vb` -> `vibe`, `vb cd` -> `vibing`: a multi-key followup replaces the
        // root's output (and its trailing smart space) with its own.
        let m = model(
            SmartSpace::Full,
            vec![root(false, "bv", "vibe", vec![child(false, "cd", "vibing", vec![])])],
        );
        let m = apply(m, &chord(Target::Root(0), "bv"));
        assert_eq!("vibe ", vis(&m));
        // Followup pressed with both keys -> prior word + space erased, replaced.
        let m = apply(m, &chord(Target::Followup(0), "cd"));
        assert_eq!("vibing ", vis(&m));
    }

    #[test]
    fn ref_suppress_multi_key_followup_order_dependent() {
        // With >1 key a followup CAN be press-order-suppressed: its first def key
        // is the minimum char ('c'). Pressing 'd' first (!= def) suppresses the
        // trailing space; pressing 'c' first keeps it.
        let mk = || {
            model_suppress(
                SmartSpace::AddOnly,
                SuppressSpace::NotFirstDefKey,
                vec![root(false, "ab", "day", vec![child(false, "cd", "Monday", vec![])])],
            )
        };
        let m = apply(mk(), &chord(Target::Root(0), "ab"));
        // Followup pressed 'd' first (!= def 'c') -> space suppressed.
        let suppressed = apply(m.clone(), &chord(Target::Followup(0), "dc"));
        assert_eq!("Monday", vis(&suppressed));
        // Followup pressed 'c' first (== def) -> space kept.
        let kept = apply(m, &chord(Target::Followup(0), "cd"));
        assert_eq!("Monday ", vis(&kept));
    }

    #[test]
    fn ref_leading_space_followup_first_def_key_is_space() {
        // ` w  a` Washington case: a leading-space followup's first def key is the
        // space. Pressing space first keeps the trailing space; a letter first
        // suppresses it.
        let mk = || {
            model_suppress(
                SmartSpace::AddOnly,
                SuppressSpace::NotFirstDefKey,
                vec![root(true, "b", "wash", vec![child(true, "a", "Washington", vec![])])],
            )
        };
        let m = apply(mk(), &chord(Target::Root(0), "b "));
        // Followup keys are {a, space}; press 'a' first (!= def space) -> suppress.
        let suppressed = apply(m.clone(), &chord(Target::Followup(0), "a "));
        assert_eq!("Washington", vis(&suppressed));
        // Press space first (== def) -> trailing space kept.
        let kept = apply(m, &chord(Target::Followup(0), " a"));
        assert_eq!("Washington ", vis(&kept));
    }

    #[test]
    fn ref_literal_disables_then_idle_reenables() {
        let m = model(SmartSpace::None, vec![root(false, "a", "X", vec![])]);
        // Type a non-chord literal: appended, zippy goes to WaitEnable.
        let m = apply(m, &KanataTransition::Literal { key: 'z' });
        assert_eq!("z", vis(&m));
        assert_eq!(Enabled::WaitEnable, m.enabled);
        // A chord while WaitEnable does not fire: passthrough of its keys.
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("za", vis(&m));
        // A full idle re-enables; now the chord fires.
        let m = apply(m, &KanataTransition::Idle { ms: WAIT + 10 });
        assert_eq!(Enabled::Enabled, m.enabled);
        let m = apply(m, &chord(Target::Root(0), "a"));
        assert_eq!("zaX", vis(&m));
    }
}

// ===========================================================================
// Second feature module: tap-hold (construction oracle)
// ===========================================================================
//
// Demonstrates the state-machine harness generalising beyond zippychord, and is
// the seam toward the real goal — *interaction* testing (see ZIPPY_PBT_NOTES.md).
//
// Two-oracle recap: zippychord splits its oracle (the `ChordExpansion`
// transition carries the expansion; the reference carries placement). Tap-hold
// here uses the other half on its own — a pure **construction oracle**. Each
// transition is a gesture kept in the *unambiguous timing interior*: released
// well before the hold timeout (=> tap) or held well past it (=> hold). So the
// expected output is known by construction with NO timing model — the same
// "stay inside the deadline" trick the chord generator uses, applied to the
// tap/hold decision boundary instead of the chord deadline.
//
// Observable: the normalised output **event stream** — the ordered ↓/↑ of output
// keys (`event_seq`), shared with the zippychord test. Timing is intentionally
// not pinned (it is impl detail); only the key-press order is asserted.
//
// Why tap-hold is the chosen second feature: it is exactly the construct the
// zippychord PBT is blind to. The PBT builds its SUT from a trivial passthrough
// layout, so layout output reaches zippychord immediately and identically
// regardless of press order; once a chord-participating key is a tap-hold (or
// layer) key the layout *delays* that key's tap output by an order/timing-
// dependent amount, which is where the known press-order bugs live. Hosting
// tap-hold in this harness is the prerequisite for generating a keymap where a
// single key is both a tap-hold action and a chord participant (the interaction
// the construction oracles can set up but only the invariants can judge).

const TH_INPUTS: &[char] = &['a', 'b', 'c'];
const TH_TAP_OUTS: &[char] = &['d', 'e', 'f'];
const TH_HOLD_OUTS: &[char] = &['x', 'y', 'z'];
// Fixed timeouts. tap=0 (eager tap) keeps the tap path simple; hold=200 is the
// decision boundary the gestures stay well clear of in both directions.
const TH_TAP_TIMEOUT: u16 = 0;
const TH_HOLD_TIMEOUT: u16 = 200;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct ThKey {
    input: char,
    tap_out: char,
    hold_out: char,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ThModel {
    keys: Vec<ThKey>,
    /// Accumulated expected output stream: (is_down, output char), known by
    /// construction from each gesture's tap/hold branch.
    expected: Vec<(bool, char)>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ThTransition {
    /// Press key `i` and release it before the hold timeout => tap output.
    Tap(usize),
    /// Press key `i` and hold past the hold timeout, then release => hold output.
    Hold(usize),
    /// Pure idle; emits nothing.
    Idle(u16),
}

impl ThModel {
    fn cfg_string(&self) -> String {
        let src = self
            .keys
            .iter()
            .map(|k| k.input.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let layer = self
            .keys
            .iter()
            .map(|k| {
                format!(
                    "(tap-hold {TH_TAP_TIMEOUT} {TH_HOLD_TIMEOUT} {} {})",
                    k.tap_out, k.hold_out
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("(defsrc {src})(deflayer base {layer})")
    }
}

/// Normalise an output event string to the ordered (is_down, char) stream,
/// dropping timing. Output keys are single letters here, so `key_to_char`
/// decodes them directly. Shared observable with the zippychord net-text oracle.
fn event_seq(events: &str) -> Vec<(bool, char)> {
    let mut v = Vec::new();
    for tok in events.split_whitespace() {
        if let Some(name) = tok.strip_prefix("out:↓") {
            if let Some(c) = key_to_char(name) {
                v.push((true, c));
            }
        } else if let Some(name) = tok.strip_prefix("out:↑") {
            if let Some(c) = key_to_char(name) {
                v.push((false, c));
            }
        }
    }
    v
}

pub struct ThRef;

impl ReferenceStateMachine for ThRef {
    type State = ThModel;
    type Transition = ThTransition;

    fn init_state() -> BoxedStrategy<Self::State> {
        (1usize..=TH_INPUTS.len())
            .prop_map(|n| ThModel {
                keys: (0..n)
                    .map(|i| ThKey {
                        input: TH_INPUTS[i],
                        tap_out: TH_TAP_OUTS[i],
                        hold_out: TH_HOLD_OUTS[i],
                    })
                    .collect(),
                expected: vec![],
            })
            .boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        let n = state.keys.len();
        let idxs: Vec<usize> = (0..n).collect();
        let tap = proptest::sample::select(idxs.clone()).prop_map(ThTransition::Tap);
        let hold = proptest::sample::select(idxs).prop_map(ThTransition::Hold);
        let idle = (1u16..=5).prop_map(ThTransition::Idle);
        prop_oneof![4 => tap, 4 => hold, 1 => idle].boxed()
    }

    fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State {
        match transition {
            ThTransition::Tap(i) => {
                let c = state.keys[*i].tap_out;
                state.expected.push((true, c));
                state.expected.push((false, c));
            }
            ThTransition::Hold(i) => {
                let c = state.keys[*i].hold_out;
                state.expected.push((true, c));
                state.expected.push((false, c));
            }
            ThTransition::Idle(_) => {}
        }
        state
    }

    fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool {
        // Guard shrinking: the dictionary (key count) can shrink independently of
        // a transition's stored index, so reject indices that fell out of range.
        match transition {
            ThTransition::Tap(i) | ThTransition::Hold(i) => *i < state.keys.len(),
            ThTransition::Idle(_) => true,
        }
    }
}

pub struct ThSut {
    kanata: Kanata,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for ThSut {
    fn drop(&mut self) {
        crate::PRESSED_KEYS.lock().clear();
    }
}

impl ThSut {
    fn press(&mut self, c: char) {
        let o = osc_of(c);
        self.kanata
            .handle_input_event(&KeyEvent::new(o, KeyValue::Press))
            .unwrap();
        pressed_insert(o);
    }
    fn release(&mut self, c: char) {
        let o = osc_of(c);
        self.kanata
            .handle_input_event(&KeyEvent::new(o, KeyValue::Release))
            .unwrap();
        pressed_remove(o);
    }
    fn tick(&mut self, ms: u16) {
        self.kanata.tick_ms(ms as u128, &None).unwrap();
    }
}

impl StateMachineTest for ThSut {
    type SystemUnderTest = ThSut;
    type Reference = ThRef;

    fn init_test(ref_state: &ThModel) -> Self::SystemUnderTest {
        let guard = match CFG_PARSE_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        crate::PRESSED_KEYS.lock().clear();
        let kanata = Kanata::new_from_str(&ref_state.cfg_string(), FxHashMap::default())
            .expect("generated tap-hold cfg must parse");
        ThSut {
            kanata,
            _guard: guard,
        }
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        ref_state: &ThModel,
        transition: ThTransition,
    ) -> Self::SystemUnderTest {
        match &transition {
            ThTransition::Tap(i) => {
                let c = ref_state.keys[*i].input;
                state.press(c);
                state.tick(5); // released well before hold timeout => tap
                state.release(c);
                state.tick(TH_HOLD_TIMEOUT + 50); // settle past the boundary
            }
            ThTransition::Hold(i) => {
                let c = ref_state.keys[*i].input;
                state.press(c);
                state.tick(TH_HOLD_TIMEOUT + 5); // held past hold timeout => hold
                state.release(c);
                state.tick(50);
            }
            ThTransition::Idle(ms) => {
                state.tick(*ms);
            }
        }
        let raw = state.kanata.kbd_out.outputs.events.join(" ");
        // Same shared catalog as the zippychord slice, selected by this slice's
        // components: the output stream and the layout/layer resolution state.
        let present = capset(&[Cap::OutputKeyState, Cap::LayoutState]);
        let quiescent = crate::PRESSED_KEYS.lock().is_empty();
        // No VisibleText component here, so the text-replay invariants
        // (no_redundant_prefix_delete / no_delete_into_void) are deselected; the
        // per-step fields are unused and left empty.
        let ctx = InvCtx {
            events: &raw,
            step_events: "",
            prev_text: "",
            quiescent,
        };
        if let Err((id, e)) = run_invariants(&present, &ctx) {
            panic!(
                "invariant `{id}` violated: {e}\n  transition: {transition:?}\n  cfg: {}\n  raw: {raw}",
                ref_state.cfg_string(),
            );
        }
        // Construction oracle: the accumulated output stream must match exactly.
        let got = event_seq(&raw);
        assert_eq!(
            ref_state.expected, got,
            "\n  transition: {:?}\n  cfg: {}\n  raw: {}",
            transition,
            ref_state.cfg_string(),
            raw
        );
        state
    }
}

prop_state_machine_persisted! {
    #![proptest_config(Config { cases: 256, .. Config::default() })]
    #[test]
    fn taphold_state_machine(sequential 1..16 => ThSut);
}

// ===========================================================================
// Interaction prototype: tap-hold x zippychord, judged by a determinism oracle
// ===========================================================================
//
// The two feature modules above each predict their isolated output. Their
// *interaction* is where the construction oracles run out: when a single key is
// both a tap-hold action and a zippychord chord participant, the layout delays
// that key's tap output by an order/timing-dependent amount, and the combined
// output is no longer a pure function of the gesture that either module can
// state. The framework still tests it — with a *metamorphic* oracle that needs
// no prediction: a chord is a set, so pressing its keys in either micro-order is
// the same physical gesture and MUST produce the same visible text.
//
// This is the harness reproduction of the press-order bug (the pinned regression
// `zippychord_sim_tests::sim_zippy_taphold_chord_press_order_dependent`): space is
// a 200ms tap-hold thumb key, the chord is leading-space ` n`->`no`, and
// `n`-first vs `space`-first used to diverge. The test asserts the *intended*
// invariant (a chord is a set ⇒ press order must not change the output). It is now
// GREEN: the bug is fixed by freezing the zippychord chord deadline while the
// layout is still deferring a tap-hold decision (see `zchd_tick`). It guards
// exactly the deferred interaction dimension this prototype exists to reach.
fn sim_zippy_file(cfg: &str, input: &str, content: &str) -> String {
    let mut fc = FxHashMap::default();
    fc.insert("file".to_string(), content.to_string());
    super::simulate_with_file_content(cfg, input, fc)
}

proptest! {
    #![proptest_config(Config { cases: 64, .. Config::default() })]
    #[test]
    fn interaction_taphold_zippy_order_independent(
        deadline in 10u16..=80,
        hold_gap in 5u16..=40,
    ) {
        // `spc` is a tap-hold thumb key (its tap output is delayed); it also
        // participates in the leading-space chord ` n`->`no`. `n` is plain.
        let cfg = format!(
            "(defsrc spc n)\
             (deflayer base (tap-hold 200 200 spc (layer-while-held l2)) n)\
             (deflayer l2 spc n)\
             (defzippy file on-first-press-chord-deadline {deadline} \
              idle-reactivate-time 100 smart-space full)"
        );
        let content = "\n n\tno\n";

        let space_first = net_text(&sim_zippy_file(
            &cfg,
            &format!("d:spc d:n t:{hold_gap} u:spc u:n t:300"),
            content,
        ));
        let n_first = net_text(&sim_zippy_file(
            &cfg,
            &format!("d:n d:spc t:{hold_gap} u:n u:spc t:300"),
            content,
        ));

        // A chord is a set: the two press orders are the same physical gesture
        // and must yield the same visible text.
        prop_assert_eq!(
            &space_first, &n_first,
            "press-order changed the chord output (deadline={}, hold_gap={}): \
             space-first={:?} n-first={:?}",
            deadline, hold_gap, space_first, n_first
        );
    }
}

// ---------------------------------------------------------------------------
// Catalog selection self-tests: guard against a slice going green only because
// every property touching its weak spot was deselected. Assert each slice's
// selected set is non-empty and as expected, no invariant has an empty positive
// footprint, and a narrow slice's set is a subset of a wider one's.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod catalog_selection_tests {
    use super::*;

    // The component sets each live slice declares (kept in sync with the SUTs).
    // The coupled `Sut` hosts zippy + tap-hold over one keymap, so it declares all
    // four; the standalone `ThSut` is tap-hold only.
    fn coupled_caps() -> CapSet {
        capset(&[
            Cap::OutputKeyState,
            Cap::VisibleText,
            Cap::ZippyState,
            Cap::LayoutState,
        ])
    }
    fn taphold_caps() -> CapSet {
        capset(&[Cap::OutputKeyState, Cap::LayoutState])
    }

    #[test]
    fn no_invariant_has_empty_positive_footprint() {
        for inv in INVARIANTS {
            assert!(
                !inv.needs_pos.is_empty(),
                "invariant `{}` observes nothing real (empty positive footprint)",
                inv.id
            );
        }
    }

    #[test]
    fn each_slice_selects_nonempty_expected() {
        for (name, caps) in [("coupled", coupled_caps()), ("taphold", taphold_caps())] {
            let ids = selected_ids(&caps);
            assert!(!ids.is_empty(), "slice `{name}` selected no invariants");
            assert!(
                ids.contains(&"no_double_press"),
                "slice `{name}` must select the universal key-state invariant"
            );
        }
    }

    #[test]
    fn empty_slice_selects_nothing() {
        // No components present => no output to judge => nothing selected.
        assert!(selected_ids(&capset(&[])).is_empty());
    }

    #[test]
    fn planted_violation_is_caught_when_selected_and_honestly_skipped_when_absent() {
        // A double-press the key-state invariant must reject.
        let bad = InvCtx {
            events: "out:↓A out:↓A",
            quiescent: true,
            step_events: "",
            prev_text: "",
        };
        // Selected (OutputKeyState present) => caught with teeth.
        assert!(
            run_invariants(&coupled_caps(), &bad).is_err(),
            "selected key-state invariant failed to catch a planted double-press"
        );
        // Absent (no OutputKeyState) => the invariant is *deselected*, so the run
        // is vacuously ok. This is honest non-selection, NOT a stubbed/faked pass:
        // a slice without the output-stream component genuinely cannot judge it.
        assert!(run_invariants(&capset(&[Cap::VisibleText]), &bad).is_ok());
    }

    #[test]
    fn planted_dangling_key_is_caught_only_at_quiescence() {
        // A held key with no release. At rest it is a clean-release violation;
        // mid-gesture (not quiescent) a held key is legitimate, so it is allowed.
        let held = "out:↓A";
        let at_rest = InvCtx {
            events: held,
            quiescent: true,
            step_events: "",
            prev_text: "",
        };
        let mid_gesture = InvCtx {
            events: held,
            quiescent: false,
            step_events: "",
            prev_text: "",
        };
        assert!(
            run_invariants(&coupled_caps(), &at_rest).is_err(),
            "clean_release failed to catch a key still held at rest"
        );
        assert!(
            run_invariants(&coupled_caps(), &mid_gesture).is_ok(),
            "a key held mid-gesture must not be flagged"
        );
    }

    #[test]
    fn redundant_prefix_delete_detector_flags_eager_echo_waste() {
        // Echoed "a", then the chord backspaces it and retypes "abc" — the shared
        // prefix "a" was deleted and retyped (the real zippychord echo waste). Net
        // text is the correct "abc", so the net-text oracle is blind; the detector
        // sees the 1 redundant delete.
        let wasteful = "out:↓a out:↑a out:↓BSpace out:↑BSpace \
                        out:↓a out:↑a out:↓b out:↑b out:↓c out:↑c";
        assert_eq!(
            1,
            redundant_prefix_deletes("", wasteful),
            "detector failed to flag an echoed-prefix delete-and-retype"
        );
    }

    #[test]
    fn necessary_delete_is_not_redundant_and_void_is_caught() {
        // Replacing "b" with a *different* char "c" is necessary, not redundant.
        let necessary = "out:↓a out:↑a out:↓b out:↑b out:↓BSpace out:↑BSpace out:↓c out:↑c";
        assert_eq!(0, redundant_prefix_deletes("", necessary));

        // A backspace on an empty buffer deletes into the void — a legality
        // violation the live catalog must still catch.
        let into_void = "out:↓BSpace out:↑BSpace";
        let bad = InvCtx {
            events: into_void,
            quiescent: true,
            step_events: into_void,
            prev_text: "",
        };
        assert!(
            run_invariants(&coupled_caps(), &bad).is_err(),
            "no_delete_into_void failed to catch a backspace on an empty buffer"
        );
    }

    #[test]
    fn disabling_an_invariant_skips_it() {
        // The env-var disable list parses and is honored by the filtered runner.
        // Tested via the pure helpers (not by mutating the process env, which would
        // race other tests).
        assert_eq!(
            parse_disabled_list(" no_double_press , , no_delete_into_void "),
            ["no_double_press", "no_delete_into_void"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
        // Press A twice, release once: violates ONLY no_double_press (the key is
        // released, so clean_release is satisfied; no text ops, so the replay
        // invariants pass). Disabling no_double_press must flip the run to ok.
        let double_press = InvCtx {
            events: "out:↓A out:↓A out:↑A",
            quiescent: true,
            step_events: "",
            prev_text: "",
        };
        // Selected and not disabled => caught.
        assert!(run_invariants_filtered(&coupled_caps(), &double_press, &BTreeSet::new()).is_err());
        // Disabled => skipped, run is ok despite the planted double-press.
        let disabled = parse_disabled_list("no_double_press");
        assert!(run_invariants_filtered(&coupled_caps(), &double_press, &disabled).is_ok());
    }

    #[test]
    fn coincidental_tail_match_is_not_redundant() {
        // Regression for a false positive: replacing "bcbb" (peak "bcbbc" after an
        // eager key) with " aab". The two share NO common prefix (' ' vs 'b'), but
        // both happen to have 'b' at index 3. In a backspace-only model you cannot
        // keep index 3 without keeping 0..2, so deleting it is NECESSARY, not waste.
        // The detector must measure the common prefix, not per-position coincidence.
        let stream = "out:↓C out:↑C \
                      out:↓BSpace out:↑BSpace out:↓BSpace out:↑BSpace out:↓BSpace out:↑BSpace \
                      out:↓BSpace out:↑BSpace out:↓BSpace out:↑BSpace \
                      out:↓Space out:↑Space out:↓A out:↑A out:↓A out:↑A out:↓B out:↑B";
        assert_eq!(
            0,
            redundant_prefix_deletes("bcbb", stream),
            "coincidental tail match wrongly counted as a redundant prefix delete"
        );
    }

    #[test]
    fn output_only_slice_is_subset_of_feature_slices() {
        // The universal read-only slice's selection must be a subset of any
        // richer slice's (adding components can only add invariants).
        let base = selected_ids(&capset(&[Cap::OutputKeyState]));
        for caps in [coupled_caps(), taphold_caps()] {
            let wide: BTreeSet<_> = selected_ids(&caps).into_iter().collect();
            assert!(
                base.iter().all(|id| wide.contains(id)),
                "narrowing changed selection non-monotonically"
            );
        }
    }

    #[test]
    fn coupled_slice_subsumes_standalone_taphold_catalog() {
        // The deletion safety-gate for the standalone `taphold_state_machine`:
        // the coupled slice must select at least every catalog invariant the
        // standalone does. It does (LayoutState ⊆ coupled caps). NOTE the
        // standalone is nonetheless KEPT — its teeth are not in the catalog but in
        // its distinct observable (the lower-level event stream, `event_seq`) and
        // its tap≠self coverage, which the coupled net-text model does not provide.
        // If those are ever folded into the coupled model, this gate licenses the
        // deletion.
        let coupled: BTreeSet<_> = selected_ids(&coupled_caps()).into_iter().collect();
        for id in selected_ids(&taphold_caps()) {
            assert!(
                coupled.contains(id),
                "coupled slice does not subsume standalone catalog invariant `{id}`"
            );
        }
    }
}
