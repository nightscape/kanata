#[cfg(feature = "zippychord")]
fn parse_zippy_inner(
    exprs: &[SExpr],
    s: &ParserState,
    f: &mut FileContentProvider,
) -> Result<(ZchPossibleChords, ZchConfig)> {
    use crate::anyhow_expr;
    use crate::subset::GetOrIsSubsetOfKnownKey::*;

    if exprs.len() < 2 {
        bail_expr!(
            &exprs[0],
            "There must be a filename following the zippy definition.\nFound {}",
            exprs.len() - 1
        );
    }

    let Some(file_name) = exprs[1].atom(s.vars()) else {
        bail_expr!(&exprs[1], "Filename must be a string, not a list.");
    };

    let mut config = ZchConfig::default();

    const KEY_NAME_MAPPINGS: &str = "output-character-mappings";
    const IDLE_REACTIVATE_TIME: &str = "idle-reactivate-time";
    const CHORD_DEADLINE: &str = "on-first-press-chord-deadline";
    const SMART_SPACE: &str = "smart-space";
    const SMART_SPACE_PUNCTUATION: &str = "smart-space-punctuation";

    let mut idle_reactivate_time_seen = false;
    let mut key_name_mappings_seen = false;
    let mut chord_deadline_seen = false;
    let mut smart_space_seen = false;
    let mut smart_space_punctuation_seen = false;
    let mut smart_space_punctuation_val_expr = None;

    let mut user_cfg_char_to_output: HashMap<char, ZchOutput> = HashMap::default();
    let mut pairs = exprs[2..].chunks_exact(2);
    for pair in pairs.by_ref() {
        let config_name = &pair[0];
        let config_value = &pair[1];

        match config_name.atom(s.vars()).ok_or_else(|| {
            anyhow_expr!(
                config_name,
                "A configuration name must be a string, not a list"
            )
        })? {
            IDLE_REACTIVATE_TIME => {
                if idle_reactivate_time_seen {
                    bail_expr!(
                        config_name,
                        "This is the 2nd instance; it can only be defined once"
                    );
                }
                idle_reactivate_time_seen = true;
                config.zch_cfg_ticks_wait_enable =
                    parse_u16(config_value, s, IDLE_REACTIVATE_TIME)?;
            }

            CHORD_DEADLINE => {
                if chord_deadline_seen {
                    bail_expr!(
                        config_name,
                        "This is the 2nd instance; it can only be defined once"
                    );
                }
                chord_deadline_seen = true;
                config.zch_cfg_ticks_chord_deadline = parse_u16(config_value, s, CHORD_DEADLINE)?;
            }

            SMART_SPACE => {
                if smart_space_seen {
                    bail_expr!(
                        config_name,
                        "This is the 2nd instance; it can only be defined once"
                    );
                }
                smart_space_seen = true;
                config.zch_cfg_smart_space = config_value
                    .atom(s.vars())
                    .and_then(|val| match val {
                        "none" => Some(ZchSmartSpaceCfg::Disabled),
                        "full" => Some(ZchSmartSpaceCfg::Full),
                        "add-space-only" => Some(ZchSmartSpaceCfg::AddSpaceOnly),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        anyhow_expr!(&config_value, "Must be: none | full | add-space-only")
                    })?;
            }

            SMART_SPACE_PUNCTUATION => {
                if smart_space_punctuation_seen {
                    bail_expr!(
                        config_name,
                        "This is the 2nd instance; it can only be defined once"
                    );
                }
                smart_space_punctuation_seen = true;
                // Need to save and parse this later since it makes use of KEY_NAME_MAPPINGS.
                smart_space_punctuation_val_expr = Some(config_value);
            }

            KEY_NAME_MAPPINGS => {
                if key_name_mappings_seen {
                    bail_expr!(
                        config_name,
                        "This is the 2nd instance; it can only be defined once"
                    );
                }
                key_name_mappings_seen = true;
                let mut mappings = config_value
                    .list(s.vars())
                    .ok_or_else(|| {
                        anyhow_expr!(
                            config_value,
                            "{KEY_NAME_MAPPINGS} must be followed by a list"
                        )
                    })?
                    .chunks_exact(2);

                for mapping_pair in mappings.by_ref() {
                    let input = mapping_pair[0]
                        .atom(None)
                        .ok_or_else(|| {
                            anyhow_expr!(&mapping_pair[0], "key mapping does not use lists")
                        })?
                        .trim_atom_quotes();
                    if input.chars().count() != 1 {
                        bail_expr!(&mapping_pair[0], "Inputs should be exactly one character");
                    }
                    let input_char = input.chars().next().expect("count is 1");

                    let output = mapping_pair[1].atom(s.vars()).ok_or_else(|| {
                        anyhow_expr!(&mapping_pair[1], "key mapping does not use lists")
                    })?;
                    let (output_mods, output_key) = parse_mod_prefix(output)?;
                    if output_mods.contains(&KeyCode::LShift)
                        && output_mods.contains(&KeyCode::RShift)
                    {
                        bail_expr!(
                            &mapping_pair[1],
                            "Both shifts are used which is redundant, use only one."
                        );
                    }
                    if output_mods
                        .iter()
                        .any(|m| !matches!(m, KeyCode::LShift | KeyCode::RShift | KeyCode::RAlt))
                    {
                        bail_expr!(&mapping_pair[1], "Only S- and AG- are supported.");
                    }
                    let output_osc = str_to_oscode(output_key)
                        .ok_or_else(|| anyhow_expr!(&mapping_pair[1], "unknown key name"))?;
                    let output = match output_mods.len() {
                        0 => ZchOutput::Lowercase(output_osc),
                        1 => match output_mods[0] {
                            KeyCode::LShift | KeyCode::RShift => ZchOutput::Uppercase(output_osc),
                            KeyCode::RAlt => ZchOutput::AltGr(output_osc),
                            _ => unreachable!("forbidden by earlier parsing"),
                        },
                        2 => ZchOutput::ShiftAltGr(output_osc),
                        _ => {
                            unreachable!("contains at most: altgr and one of the shifts")
                        }
                    };
                    if user_cfg_char_to_output.insert(input_char, output).is_some() {
                        bail_expr!(&mapping_pair[0], "Duplicate character, not allowed");
                    }
                }

                let rem = mappings.remainder();
                if !rem.is_empty() {
                    bail_expr!(&rem[0], "zippy input is missing its output mapping");
                }
            }
            _ => bail_expr!(config_name, "Unknown zippy configuration name"),
        }
    }

    let rem = pairs.remainder();
    if !rem.is_empty() {
        bail_expr!(&rem[0], "zippy config name is missing its value");
    }

    if let Some(val) = smart_space_punctuation_val_expr {
        config.zch_cfg_smart_space_punctuation = val
            .list(s.vars())
            .ok_or_else(|| {
                anyhow_expr!(val, "{SMART_SPACE_PUNCTUATION} must be followed by a list")
            })?
            .iter()
            .try_fold(vec![], |mut puncs, punc_expr| -> Result<Vec<ZchOutput>> {
                let punc = punc_expr
                    .atom(s.vars())
                    .ok_or_else(|| anyhow_expr!(&punc_expr, "Lists are not allowed"))?;

                if punc.chars().count() == 1 {
                    let c = punc.chars().next().expect("checked count above");
                    if let Some(out) = user_cfg_char_to_output.get(&c) {
                        puncs.push(*out);
                        return Ok(puncs);
                    }
                }

                let osc = str_to_oscode(punc)
                    .ok_or_else(|| anyhow_expr!(&punc_expr, "Unknown key name"))?;
                puncs.push(ZchOutput::Lowercase(osc));

                Ok(puncs)
            })?
            .into_iter()
            .collect();
        config.zch_cfg_smart_space_punctuation.shrink_to_fit();
    }

    // process zippy file
    let input_data = f
        .get_file_content(file_name.as_ref())
        .map_err(|e| anyhow_expr!(&exprs[1], "Failed to read file:\n{e}"))?;
    let res = input_data
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim().starts_with("//"))
        .try_fold(
            Arc::new(Mutex::new(ZchPossibleChords(SubsetMap::ssm_new()))),
            |zch, (line_number, line)| {
                let Some((input, output)) = line.split_once('\t') else {
                    bail_expr!(
                        &exprs[1],
                        "Input and output are separated by a tab, but found no tab:\n{}: {line}",
                        line_number + 1
                    );
                };
                if input.is_empty() {
                    bail_expr!(
                        &exprs[1],
                        "No input defined; line must not begin with a tab:\n{}: {line}",
                        line_number + 1
                    );
                }

                let mut char_buf: [u8; 4] = [0; 4];
                let output = {
                    output
                        .chars()
                        .try_fold(vec![], |mut zch_output, out_char| -> Result<_> {
                            if let Some(out) = user_cfg_char_to_output.get(&out_char) {
                                zch_output.push(*out);
                                return Ok(zch_output);
                            }

                            let out_key = out_char.to_lowercase().next().unwrap();
                            let key_name = out_key.encode_utf8(&mut char_buf);
                            let osc = match key_name as &str {
                                " " => OsCode::KEY_SPACE,
                                _ => str_to_oscode(key_name).ok_or_else(|| {
                                    anyhow_expr!(
                                        &exprs[1],
                                        "Unknown output key name '{}':\n{}: {line}",
                                        out_char,
                                        line_number + 1,
                                    )
                                })?,
                            };
                            let out = match out_char.is_uppercase() {
                                true => ZchOutput::Uppercase(osc),
                                false => ZchOutput::Lowercase(osc),
                            };
                            zch_output.push(out);
                            Ok(zch_output)
                        })?
                        .into_boxed_slice()
                };
                let mut input_left_to_parse = input;
                let mut chord_chars;
                let mut input_chord = ZchInputKeys::zchik_new();
                let mut is_space_included;
                let mut possible_chords_map = zch.clone();
                let mut next_map: Option<Arc<Mutex<_>>>;

                while !input_left_to_parse.is_empty() {
                    input_chord.zchik_clear();

                    // Check for a starting space.
                    (is_space_included, input_left_to_parse) =
                        match input_left_to_parse.strip_prefix(' ') {
                            None => (false, input_left_to_parse),
                            Some(i) => (true, i),
                        };
                    if is_space_included {
                        input_chord.zchik_insert(OsCode::KEY_SPACE);
                    }

                    // Parse chord until next space.
                    (chord_chars, input_left_to_parse) = match input_left_to_parse.split_once(' ') {
                        Some(split) => split,
                        None => (input_left_to_parse, ""),
                    };

                    chord_chars
                        .chars()
                        .try_fold((), |_, chord_char| -> Result<()> {
                            let key_name = chord_char.encode_utf8(&mut char_buf);
                            let osc = str_to_oscode(key_name).ok_or_else(|| {
                                anyhow_expr!(
                                    &exprs[1],
                                    "Unknown input key name: '{key_name}':\n{}: {line}",
                                    line_number + 1
                                )
                            })?;
                            input_chord.zchik_insert(osc);
                            Ok(())
                        })?;

                    let output_for_input_chord = possible_chords_map
                        .lock()
                        .0
                        .ssm_get_or_is_subset_ksorted(input_chord.zchik_keys());
                    match (input_left_to_parse.is_empty(), output_for_input_chord) {
                        (true, HasValue(_)) => {
                            bail_expr!(
                            &exprs[1],
                            "Found duplicate input chord, which is disallowed {input}:\n{}: {line}",
                            line_number + 1
                        );
                        }
                        (true, _) => {
                            possible_chords_map.lock().0.ssm_insert_ksorted(
                                input_chord.zchik_keys(),
                                Arc::new(ZchChordOutput {
                                    zch_output: output,
                                    zch_followups: None,
                                }),
                            );
                            break;
                        }
                        (false, HasValue(next_nested_map)) => {
                            match &next_nested_map.zch_followups {
                                None => {
                                    let map = Arc::new(Mutex::new(ZchPossibleChords(
                                        SubsetMap::ssm_new(),
                                    )));
                                    next_map = Some(map.clone());
                                    possible_chords_map.lock().0.ssm_insert_ksorted(
                                        input_chord.zchik_keys(),
                                        ZchChordOutput {
                                            zch_output: next_nested_map.zch_output.clone(),
                                            zch_followups: Some(map),
                                        }
                                        .into(),
                                    );
                                }
                                Some(followup) => {
                                    next_map = Some(followup.clone());
                                }
                            }
                        }
                        (false, _) => {
                            let map = Arc::new(Mutex::new(ZchPossibleChords(SubsetMap::ssm_new())));
                            next_map = Some(map.clone());
                            possible_chords_map.lock().0.ssm_insert_ksorted(
                                input_chord.zchik_keys(),
                                Arc::new(ZchChordOutput {
                                    zch_output: Box::new([]),
                                    zch_followups: Some(map),
                                }),
                            );
                        }
                    };
                    if let Some(map) = next_map.take() {
                        possible_chords_map = map;
                    }
                }
                Ok(zch)
            },
        )?;
    Ok((
        Arc::into_inner(res).expect("no other refs").into_inner(),
        config,
    ))
}

pub(crate) fn parse_zippy(
    exprs: &[SExpr],
    s: &ParserState,
    f: &mut FileContentProvider,
) -> Result<(ZchPossibleChords, ZchConfig)> {
    parse_zippy_inner(exprs, s, f)
}

#[cfg(not(feature = "zippychord"))]
fn parse_zippy_inner(
    exprs: &[SExpr],
    _s: &ParserState,
    _f: &mut FileContentProvider,
) -> Result<(ZchPossibleChords, ZchConfig)> {
    bail_expr!(&exprs[0], "Kanata was not compiled with the \"zippychord\" feature. This configuration is unsupported")
}

