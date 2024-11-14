
#[allow(unused_variables)]
fn set_virtual_key_reference_lsp_hint(vk_name_expr: &SExpr, s: &ParserState) {
    #[cfg(feature = "lsp")]
    {
        let atom = match vk_name_expr {
            SExpr::Atom(x) => x,
            SExpr::List(_) => unreachable!("should be validated to be atom earlier"),
        };
        s.lsp_hints
            .borrow_mut()
            .reference_locations
            .virtual_key
            .push_from_atom(atom);
    }
}



pub(crate) fn parse_on_press_fake_key_op(
    ac_params: &[SExpr],
    s: &ParserState,
) -> Result<&'static KanataAction> {
    let (coord, action) = parse_fake_key_op_coord_action(ac_params, s, ON_PRESS_FAKEKEY)?;
    set_virtual_key_reference_lsp_hint(&ac_params[0], s);
    Ok(s.a.sref(Action::Custom(
        s.a.sref(s.a.sref_slice(CustomAction::FakeKey { coord, action })),
    )))
}

pub(crate) fn parse_on_release_fake_key_op(
    ac_params: &[SExpr],
    s: &ParserState,
) -> Result<&'static KanataAction> {
    let (coord, action) = parse_fake_key_op_coord_action(ac_params, s, ON_RELEASE_FAKEKEY)?;
    set_virtual_key_reference_lsp_hint(&ac_params[0], s);
    Ok(s.a.sref(Action::Custom(s.a.sref(
        s.a.sref_slice(CustomAction::FakeKeyOnRelease { coord, action }),
    ))))
}

pub(crate) fn parse_on_idle_fakekey(
    ac_params: &[SExpr],
    s: &ParserState,
) -> Result<&'static KanataAction> {
    const ERR_MSG: &str =
        "on-idle-fakekey expects three parameters:\n<fake key name> <(tap|press|release)> <idle time>\n";
    if ac_params.len() != 3 {
        bail!("{ERR_MSG}");
    }
    let y = match s
        .virtual_keys
        .get(ac_params[0].atom(s.vars()).ok_or_else(|| {
            anyhow_expr!(
                &ac_params[0],
                "{ERR_MSG}\nInvalid first parameter: a fake key name cannot be a list",
            )
        })?) {
        Some((y, _)) => *y as u16, // cast should be safe; checked in `parse_fake_keys`
        None => bail_expr!(
            &ac_params[0],
            "{ERR_MSG}\nInvalid first parameter: unknown fake key name {:?}",
            &ac_params[0]
        ),
    };
    let action = ac_params[1]
        .atom(s.vars())
        .and_then(|a| match a {
            "tap" => Some(FakeKeyAction::Tap),
            "press" => Some(FakeKeyAction::Press),
            "release" => Some(FakeKeyAction::Release),
            _ => None,
        })
        .ok_or_else(|| {
            anyhow_expr!(
                &ac_params[1],
                "{ERR_MSG}\nInvalid second parameter, it must be one of: tap, press, release",
            )
        })?;
    let idle_duration = parse_u16(&ac_params[2], s, "idle time").map_err(|mut e| {
        e.msg = format!("{ERR_MSG}\nInvalid third parameter: {}", e.msg);
        e
    })?;
    let (x, y) = get_fake_key_coords(y);
    let coord = Coord { x, y };
    set_virtual_key_reference_lsp_hint(&ac_params[0], s);
    Ok(s.a.sref(Action::Custom(s.a.sref(s.a.sref_slice(
        CustomAction::FakeKeyOnIdle(FakeKeyOnIdle {
            coord,
            action,
            idle_duration,
        }),
    )))))
}

fn parse_fake_key_op_coord_action(
    ac_params: &[SExpr],
    s: &ParserState,
    ac_name: &str,
) -> Result<(Coord, FakeKeyAction)> {
    const ERR_MSG: &str = "expects two parameters: <fake key name> <(tap|press|release|toggle)>";
    if ac_params.len() != 2 {
        bail!("{ac_name} {ERR_MSG}");
    }
    let y = match s
        .virtual_keys
        .get(ac_params[0].atom(s.vars()).ok_or_else(|| {
            anyhow_expr!(
                &ac_params[0],
                "{ac_name} {ERR_MSG}\nInvalid first parameter: a fake key name cannot be a list",
            )
        })?) {
        Some((y, _)) => *y as u16, // cast should be safe; checked in `parse_fake_keys`
        None => bail_expr!(
            &ac_params[0],
            "{ac_name} {ERR_MSG}\nInvalid first parameter: unknown fake key name {:?}",
            &ac_params[0]
        ),
    };
    let action = ac_params[1]
        .atom(s.vars())
        .and_then(|a| match a {
            "tap" => Some(FakeKeyAction::Tap),
            "press" => Some(FakeKeyAction::Press),
            "release" => Some(FakeKeyAction::Release),
            "toggle" => Some(FakeKeyAction::Toggle),
            _ => None,
        })
        .ok_or_else(|| {
            anyhow_expr!(
                &ac_params[1],
                "{ERR_MSG}\nInvalid second parameter, it must be one of: tap, press, release",
            )
        })?;
    let (x, y) = get_fake_key_coords(y);
    set_virtual_key_reference_lsp_hint(&ac_params[0], s);
    Ok((Coord { x, y }, action))
}