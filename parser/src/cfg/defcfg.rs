#[cfg(any(target_os = "linux", target_os = "macos", target_os = "unknown"))]
pub fn parse_dev(val: &SExpr) -> Result<Vec<String>> {
    Ok(match val {
        SExpr::Atom(a) => {
            let devs = parse_colon_separated_text(a.t.trim_atom_quotes());
            if devs.len() == 1 && devs[0].is_empty() {
                bail_expr!(val, "an empty string is not a valid device name or path")
            }
            devs
        }
        SExpr::List(l) => {
            let r: Result<Vec<String>> =
                l.t.iter()
                    .try_fold(Vec::with_capacity(l.t.len()), |mut acc, expr| match expr {
                        SExpr::Atom(path) => {
                            let trimmed_path = path.t.trim_atom_quotes().to_string();
                            if trimmed_path.is_empty() {
                                bail_span!(
                                    path,
                                    "an empty string is not a valid device name or path"
                                )
                            }
                            acc.push(trimmed_path);
                            Ok(acc)
                        }
                        SExpr::List(inner_list) => {
                            bail_span!(inner_list, "expected strings, found a list")
                        }
                    });

            r?
        }
    })
}