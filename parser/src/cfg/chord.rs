use itertools::Itertools;
use kanata_keyberon::chord::{ChordV2, ChordsForKey, ChordsForKeys, ReleaseBehaviour};
use rustc_hash::{FxHashMap, FxHashSet};

use std::fs;
use std::path::Path;
use std::{collections::HashMap, rc::Rc};

use crate::{anyhow_expr, bail_expr, custom_action};

use super::*;

pub(crate) fn parse_defchordv2(
    exprs: &[SExpr],
    s: &ParserState,
) -> Result<ChordsForKeys<'static, KanataCustom>> {
    let mut chunks = exprs[1..].chunks_exact(5);
    let mut chords_container = ChordsForKeys::<'static, KanataCustom> {
        mapping: FxHashMap::default(),
    };
    let all_chords = chunks
        .by_ref()
        .flat_map(|chunk| match chunk[0] {
            // Match a line like
            // (include filename.txt) () 100 all-released (layer1 layer2)
            SExpr::List(Spanned {
                t: ref exprs,
                span: _,
            }) if matches!(exprs.get(0), Some(SExpr::Atom(a)) if a.t == "include") => {
                println!("include file: {:?}", exprs.len());
                parse_chord_file(chunk, s)
            }
            SExpr::List(_) => Ok(vec![parse_single_chord(chunk, s)]),
            _ => Ok(vec![]),
        })
        .flat_map(|vec_result| vec_result.into_iter())
        .collect::<Vec<Result<_>>>();
    let unsuccessful = all_chords
        .iter()
        .filter_map(|r| r.as_ref().err())
        .collect::<Vec<_>>();
    if !unsuccessful.is_empty() {
        bail_expr!(
            &exprs[0],
            "Error parsing chord definition:\n{}",
            unsuccessful
                .iter()
                .map(|e| e.msg.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    let successful = all_chords.into_iter().filter_map(Result::ok).collect_vec();

    let mut all_participating_key_sets = FxHashSet::default();
    for chord in successful {
        if !all_participating_key_sets.insert(chord.participating_keys) {
            ParseError::new_without_span(
                "This chord has previously been defined.\n\
                Only one set of chords must exist for one key combination.",
            );
        } else {
            for pkey in chord.participating_keys.iter().copied() {
                //log::trace!("chord for key:{pkey:?} > {chord:?}");
                chords_container
                    .mapping
                    .entry(pkey)
                    .or_insert(ChordsForKey { chords: vec![] })
                    .chords
                    .push(s.a.sref(chord.clone()));
            }
        }
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        bail_expr!(
            rem.last().unwrap(),
            "Incomplete chord entry. Each chord entry must have 5 items:\n\
        particpating-keys, action, timeout, release-type, disabled-layers"
        );
    }
    Ok(chords_container)
}

fn parse_single_chord(chunk: &[SExpr], s: &ParserState) -> Result<ChordV2<'static, KanataCustom>> {
    let keys = &chunk[0];
    let key_strings = keys
        .list(s.vars())
        .map(|keys| {
            keys.iter()
                .map(|key| {
                    key.atom(s.vars()).ok_or_else(|| {
                        anyhow_expr!(key, "The first chord item must be a list of keys.")
                    })
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| anyhow_expr!(keys, "The first chord item must be a list of keys."))?
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    if key_strings.len() < 2 {
        bail_expr!(keys, "The minimum number of participating chord keys is 2");
    }
    let participants = parse_participating_keys(key_strings)?;

    let action = parse_action(&chunk[1], s)?;
    let timeout = parse_timeout(&chunk[2], s)?;
    let release_behaviour = parse_release_behaviour(&chunk[3], s)?;
    let disabled_layers = parse_disabled_layers(&chunk[4], s)?;
    let chord: ChordV2<'static, KanataCustom> = ChordV2 {
        action,
        participating_keys: s.a.sref_vec(participants.clone()),
        pending_duration: timeout,
        disabled_layers: s.a.sref_vec(disabled_layers),
        release_behaviour,
    };
    return Ok(s.a.sref(chord).clone());
}

fn parse_participating_keys(key_strings: Vec<&str>) -> Result<Vec<u16>> {
    let mut participants =
        key_strings
            .iter()
            .try_fold(vec![], |mut keys, key| -> Result<Vec<u16>> {
                let k = str_to_oscode(key).ok_or_else(|| {
                    ParseError::new_without_span(format!("Invalid keycode: '{}'", key))
                })?;
                keys.push(k.into());
                Ok(keys)
            })?;
    participants.sort();
    Ok(participants)
}

fn parse_timeout(chunk: &SExpr, s: &ParserState) -> Result<u16> {
    let timeout = parse_non_zero_u16(&chunk, s, "chord timeout")?;
    Ok(timeout)
}

fn parse_release_behaviour(
    release_behaviour_string: &SExpr,
    s: &ParserState,
) -> Result<ReleaseBehaviour> {
    let release_behaviour = release_behaviour_string
        .atom(s.vars())
        .and_then(|r| {
            Some(match r {
                "first-release" => ReleaseBehaviour::OnFirstRelease,
                "all-released" => ReleaseBehaviour::OnLastRelease,
                _ => return None,
            })
        })
        .ok_or_else(|| {
            anyhow_expr!(
                release_behaviour_string,
                "Chord release behaviour must be one of:\n\
                first-release | all-released"
            )
        })?;
    Ok(release_behaviour)
}

fn parse_disabled_layers(disabled_layers: &SExpr, s: &ParserState) -> Result<Vec<u16>> {
    let disabled_layers = disabled_layers
        .list(s.vars())
        .map(|dl| {
            dl.iter()
                .try_fold(vec![], |mut layers, layer| -> Result<Vec<u16>> {
                    let l_idx = layer
                        .atom(s.vars())
                        .and_then(|l| s.layer_idxs.get(l))
                        .ok_or_else(|| anyhow_expr!(layer, "Not a known layer name."))?;
                    layers.push((*l_idx) as u16);
                    Ok(layers)
                })
        })
        .ok_or_else(|| {
            anyhow_expr!(
                disabled_layers,
                "Disabled layers must be a list of layer names"
            )
        })??;
    Ok(disabled_layers)
}
fn parse_chord_file(
    chunk: &[SExpr],
    s: &ParserState,
) -> Result<Vec<Result<ChordV2<'static, KanataCustom>>>> {
    let file_name = chunk[0].list(s.vars()).unwrap()[1].atom(s.vars()).unwrap();
    let timeout = parse_timeout(&chunk[2], s)?;
    let release_behaviour = parse_release_behaviour(&chunk[3], s)?;
    let disabled_layers = parse_disabled_layers(&chunk[4], s)?;
    let input_data =
        fs::read_to_string(file_name).expect(format!("Unable to read file {}", file_name).as_str());
    let parsed_chords = parse_input(&input_data);
    let mapped_chords = map_to_physical_keys(
        parsed_chords,
        timeout,
        release_behaviour,
        disabled_layers,
        s,
    );
    return Ok(mapped_chords);
}

fn parse_input(input: &str) -> Vec<ChordDefinition> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with("//"))
        .filter_map(|line| {
            let caps = line.split("\t").collect::<Vec<&str>>();
            if caps.len() < 2 {
                return None;
            }
            Some(ChordDefinition {
                keys: caps[0],
                action: caps[1],
            })
        })
        .collect()
}

fn map_to_physical_keys(
    chords: Vec<ChordDefinition>,
    timeout: u16,
    release_behaviour: ReleaseBehaviour,
    disabled_layers: Vec<u16>,
    s: &ParserState,
) -> Vec<Result<ChordV2<'static, &'static &'static [&'static custom_action::CustomAction]>>> {
    let target_map = s.layers[0][0]
        .iter()
        .enumerate()
        .filter_map(|(idx, layout)| {
            layout
                .key_codes()
                .next()
                .map(|kc| kc.to_string().to_lowercase())
                .zip(
                    idx.try_into()
                        .ok()
                        .and_then(|num| OsCode::from_u16(num))
                        .map(|osc| osc.to_string().to_lowercase()),
                )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .chain(vec![(" ".to_string(), "spc".to_string())].into_iter())
        .collect::<HashMap<_, _>>();
    let postprocess_map: HashMap<String, String> = [
        ("semicolon", ";"),
        ("colon", "S-."),
        ("slash", "/"),
        ("apostrophe", "'"),
        ("dot", "."),
        (" ", "spc"),
    ]
    .iter()
    .cloned()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let process = |key: &str| {
        let key = key.to_string();
        let converted = &key; //target_map.get(&key).unwrap_or(&key);
        postprocess_map
            .get(converted)
            .map(|c| c.to_string())
            .unwrap_or_else(|| {
                if converted.chars().all(|c| c.is_uppercase()) {
                    format!("S-{}", converted.to_lowercase())
                } else {
                    converted.to_string()
                }
            })
    };

    let alias_map = s
        .aliases
        .iter()
        .filter_map(|(k, v)| {
            if k.len() != 1 {
                None
            } else {
                Some((k.to_string(), v.key_codes().collect::<Vec<KeyCode>>()))
            }
        })
        .collect::<HashMap<_, _>>();
    chords
        .into_iter()
        .map(|chord| {
            let keys = chord
                .keys
                .chars()
                .map(|c| {
                    let c = c.to_string();
                    target_map.get(&c).map(String::to_string).unwrap_or(c)
                })
                .map(|s| process(&s))
                .collect::<Vec<String>>();
            let sequence_events = chord
                .action
                .chars()
                .map(|c| process(c.to_string().as_str()))
                .flat_map(|c| {
                    parse_action_atom(
                        &Spanned {
                            t: c.to_string(),
                            span: Span::default(),
                        },
                        s,
                    )
                    .unwrap()
                    .key_codes()
                    .flat_map(|code| vec![SequenceEvent::Press(code), SequenceEvent::Release(code)])
                    .collect::<Vec<_>>()
                })
                .collect::<Vec<SequenceEvent<_>>>();
            // Now insert a SequenceEvent::Release(Shift) after the second element of sequence_events:
            let sequence_events = sequence_events
                .into_iter()
                .enumerate()
                .flat_map(|(idx, event)| {
                    if idx == 1 {
                        vec![event, SequenceEvent::Release(KeyCode::LShift), SequenceEvent::Release(KeyCode::RShift)]
                    } else {
                        vec![event]
                    }
                })
                .collect::<Vec<_>>();
            let events_slice = s.a.sref(s.a.sref(s.a.sref_vec(sequence_events)));
            let action = Action::Sequence {
                events: events_slice,
            };
            let participating_ks =
                parse_participating_keys(keys.iter().map(String::as_str).collect::<Vec<&str>>())?;
            let disabled_layers_cloned = disabled_layers.clone();
            let chord: ChordV2<'static, KanataCustom> = ChordV2 {
                participating_keys: &s.a.sref(participating_ks),
                action: &s.a.sref(action),
                pending_duration: timeout,
                disabled_layers: &s.a.sref(disabled_layers_cloned),
                release_behaviour: release_behaviour,
            };
            Ok(s.a.sref(chord).clone())
        })
        .collect()
}

// Define necessary data structures

#[derive(Debug)]
struct ChordDefinition<'a> {
    keys: &'a str,
    action: &'a str,
}
