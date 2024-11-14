//! This parses the configuration language to create a `kanata_keyberon::layout::Layout` as well as
//! associated metadata to help with processing.
//!
//! How the configuration maps to keyberon:
//!
//! If the mapped keys are defined as:
//!
//! (defsrc
//!     esc  1    2    3    4
//! )
//!
//! and the layers are:
//!
//! (deflayer one
//!     _   a    s    d    _
//! )
//!
//! (deflayer two
//!     _   a    o    e    _
//! )
//!
//! Then the keyberon layers will be as follows:
//!
//! (xx means unimportant and _ means transparent)
//!
//! layers[0] = { xx, esc, a, s, d, 4, xx... }
//! layers[1] = { xx, _  , a, s, d, _, xx... }
//! layers[2] = { xx, esc, a, o, e, 4, xx... }
//! layers[3] = { xx, _  , a, o, e, _, xx... }
//!
//! Note that this example isn't practical, but `(defsrc esc 1 2 3 4)` is used because these keys
//! are at the beginning of the array. The column index for layers is the numerical value of
//! the key from `keys::OsCode`.
//!
//! In addition, there are two versions of each layer. One version delegates transparent entries to
//! the key defined in defsrc, while the other keeps them as actually transparent. This is to match
//! the behaviour in kmonad.
//!
//! The specific values in example above applies to Linux, but the same logic applies to Windows.

pub mod sexpr;

pub(crate) mod alloc;
use alloc::*;

use crate::sequences::*;
use kanata_keyberon::chord::ChordsV2;

mod key_override;
pub use key_override::*;

mod custom_tap_hold;
use custom_tap_hold::*;

pub mod layer_opts;
use layer_opts::*;

pub mod list_actions;
use list_actions::*;

mod defcfg;
pub use defcfg::*;

mod deftemplate;
pub use deftemplate::*;

use crate::custom_action::*;
use crate::keys::*;
use crate::layers::*;

mod error;
pub use error::*;

mod fake_key;
use fake_key::*;
pub use fake_key::{FAKE_KEY_ROW, NORMAL_KEY_ROW};

mod platform;
use platform::*;

mod is_a_button;
use is_a_button::*;

mod key_outputs;
pub use key_outputs::*;

mod permutations;
use permutations::*;

mod zippychord;
pub use zippychord::*;

use crate::lsp_hints::{self, LspHints};

mod str_ext;
pub use str_ext::*;

use crate::trie::Trie;
use anyhow::anyhow;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

type HashSet<T> = rustc_hash::FxHashSet<T>;
type HashMap<K, V> = rustc_hash::FxHashMap<K, V>;

use kanata_keyberon::action::*;
use kanata_keyberon::key_code::*;
use kanata_keyberon::layout::*;
use sexpr::*;

/// Recurse through all levels of list nesting and collect into a flat list of strings.
/// Recursion is DFS, which matches left-to-right reading of the strings as they appear,
/// if everything was on a single line.
fn collect_strings(params: &[SExpr], strings: &mut Vec<String>, s: &ParserState) {
    for param in params {
        if let Some(a) = param.atom(s.vars()) {
            strings.push(a.trim_atom_quotes().to_owned());
        } else {
            // unwrap: this must be a list, since it's not an atom.
            let l = param.list(s.vars()).unwrap();
            collect_strings(l, strings, s);
        }
    }
}

#[test]
fn test_collect_strings() {
    let params = r#"(gah (squish "squash" (splish splosh) "bah mah") dah)"#;
    let params = sexpr::parse(params, "noexist").unwrap();
    let mut strings = vec![];
    collect_strings(&params[0].t, &mut strings, &ParserState::default());
    assert_eq!(
        &strings,
        &["gah", "squish", "squash", "splish", "splosh", "bah mah", "dah"]
    );
}

fn parse_push_message(ac_params: &[SExpr], s: &ParserState) -> Result<&'static KanataAction> {
    if ac_params.is_empty() {
        bail!(
             "{PUSH_MESSAGE} expects at least one item, an item can be a list or an atom, found 0, none"
        );
    }
    let message = to_simple_expr(ac_params, s);
    custom(CustomAction::PushMessage(message), &s.a)
}

fn to_simple_expr(params: &[SExpr], s: &ParserState) -> Vec<SimpleSExpr> {
    let mut result: Vec<SimpleSExpr> = Vec::new();
    for param in params {
        if let Some(a) = param.atom(s.vars()) {
            result.push(SimpleSExpr::Atom(a.trim_atom_quotes().to_owned()));
        } else {
            // unwrap: this must be a list, since it's not an atom.
            let sexps = param.list(s.vars()).unwrap();
            let value = to_simple_expr(sexps, s);
            let list = SimpleSExpr::List(value);
            result.push(list);
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimpleSExpr {
    Atom(String),
    List(Vec<SimpleSExpr>),
}

fn create_defsrc_layer() -> [KanataAction; KEYS_IN_ROW] {
    let mut layer = [KanataAction::NoOp; KEYS_IN_ROW];

    for (i, ac) in layer.iter_mut().enumerate() {
        *ac = OsCode::from_u16(i as u16)
            .map(|osc| Action::KeyCode(osc.into()))
            .unwrap_or(Action::NoOp);
    }
    // Ensure 0-index is no-op.
    layer[0] = KanataAction::NoOp;
    layer
}





fn find_chords_coords(chord_groups: &mut [ChordGroup], coord: (u8, u16), action: &KanataAction) {
    match action {
        Action::Chords(ChordsGroup { coords, .. }) => {
            for ((_, group_id), chord_keys) in coords.iter() {
                let group = &mut chord_groups[*group_id as usize];
                group.coords.push((coord, *chord_keys));
            }
        }
        Action::NoOp
        | Action::Trans
        | Action::Src
        | Action::Repeat
        | Action::KeyCode(_)
        | Action::MultipleKeyCodes(_)
        | Action::Layer(_)
        | Action::DefaultLayer(_)
        | Action::Sequence { .. }
        | Action::RepeatableSequence { .. }
        | Action::CancelSequences
        | Action::ReleaseState(_)
        | Action::Custom(_) => {}
        Action::HoldTap(HoldTapAction { tap, hold, .. }) => {
            find_chords_coords(chord_groups, coord, tap);
            find_chords_coords(chord_groups, coord, hold);
        }
        Action::OneShot(OneShot { action: ac, .. }) => {
            find_chords_coords(chord_groups, coord, ac);
        }
        Action::MultipleActions(actions) => {
            for ac in actions.iter() {
                find_chords_coords(chord_groups, coord, ac);
            }
        }
        Action::TapDance(TapDance { actions, .. }) => {
            for ac in actions.iter() {
                find_chords_coords(chord_groups, coord, ac);
            }
        }
        Action::Fork(ForkConfig { left, right, .. }) => {
            find_chords_coords(chord_groups, coord, left);
            find_chords_coords(chord_groups, coord, right);
        }
        Action::Switch(Switch { cases }) => {
            for case in cases.iter() {
                find_chords_coords(chord_groups, coord, case.1);
            }
        }
    }
}

fn fill_chords(
    chord_groups: &[&'static ChordsGroup<&&[&CustomAction]>],
    action: &KanataAction,
    s: &ParserState,
) -> Option<KanataAction> {
    match action {
        Action::Chords(ChordsGroup { coords, .. }) => {
            let ((_, group_id), _) = coords
                .iter()
                .next()
                .expect("unresolved chords should have exactly one entry");
            Some(Action::Chords(chord_groups[*group_id as usize]))
        }
        Action::NoOp
        | Action::Trans
        | Action::Repeat
        | Action::Src
        | Action::KeyCode(_)
        | Action::MultipleKeyCodes(_)
        | Action::Layer(_)
        | Action::DefaultLayer(_)
        | Action::Sequence { .. }
        | Action::RepeatableSequence { .. }
        | Action::CancelSequences
        | Action::ReleaseState(_)
        | Action::Custom(_) => None,
        Action::HoldTap(&hta @ HoldTapAction { tap, hold, .. }) => {
            let new_tap = fill_chords(chord_groups, &tap, s);
            let new_hold = fill_chords(chord_groups, &hold, s);
            if new_tap.is_some() || new_hold.is_some() {
                Some(Action::HoldTap(s.a.sref(HoldTapAction {
                    hold: new_hold.unwrap_or(hold),
                    tap: new_tap.unwrap_or(tap),
                    ..hta
                })))
            } else {
                None
            }
        }
        Action::OneShot(&os @ OneShot { action: ac, .. }) => {
            fill_chords(chord_groups, ac, s).map(|ac| {
                Action::OneShot(s.a.sref(OneShot {
                    action: s.a.sref(ac),
                    ..os
                }))
            })
        }
        Action::MultipleActions(actions) => {
            let new_actions = actions
                .iter()
                .map(|ac| fill_chords(chord_groups, ac, s))
                .collect::<Vec<_>>();
            if new_actions.iter().any(|it| it.is_some()) {
                let new_actions = new_actions
                    .iter()
                    .zip(**actions)
                    .map(|(new_ac, ac)| new_ac.unwrap_or(*ac))
                    .collect::<Vec<_>>();
                Some(Action::MultipleActions(s.a.sref(s.a.sref_vec(new_actions))))
            } else {
                None
            }
        }
        Action::TapDance(&td @ TapDance { actions, .. }) => {
            let new_actions = actions
                .iter()
                .map(|ac| fill_chords(chord_groups, ac, s))
                .collect::<Vec<_>>();
            if new_actions.iter().any(|it| it.is_some()) {
                let new_actions = new_actions
                    .iter()
                    .zip(actions)
                    .map(|(new_ac, ac)| new_ac.map(|v| s.a.sref(v)).unwrap_or(*ac))
                    .collect::<Vec<_>>();
                Some(Action::TapDance(s.a.sref(TapDance {
                    actions: s.a.sref_vec(new_actions),
                    ..td
                })))
            } else {
                None
            }
        }
        Action::Fork(&fcfg @ ForkConfig { left, right, .. }) => {
            let new_left = fill_chords(chord_groups, &left, s);
            let new_right = fill_chords(chord_groups, &right, s);
            if new_left.is_some() || new_right.is_some() {
                Some(Action::Fork(s.a.sref(ForkConfig {
                    left: new_left.unwrap_or(left),
                    right: new_right.unwrap_or(right),
                    ..fcfg
                })))
            } else {
                None
            }
        }
        Action::Switch(Switch { cases }) => {
            let mut new_cases = vec![];
            for case in cases.iter() {
                new_cases.push((
                    case.0,
                    fill_chords(chord_groups, case.1, s)
                        .map(|ac| s.a.sref(ac))
                        .unwrap_or(case.1),
                    case.2,
                ));
            }
            Some(Action::Switch(s.a.sref(Switch {
                cases: s.a.sref_vec(new_cases),
            })))
        }
    }
}
