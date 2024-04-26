use kanata_parser::cfg::{self, Cfg, MResult};
use kanata_state_machine::OsCode;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn parse_kbd_file(file_path: &str) -> MResult<Cfg> {
    let path = Path::new(file_path);
    return cfg::new_from_file(path);
}
fn main() {
    let input_data = fs::read_to_string("cfg_samples/chords.txt").expect("Unable to read file");
    let parsed_config =
        parse_kbd_file("cfg_samples/bone_mac.kbd").expect("Unable to parse kbd file");

    let target_map = parsed_config.layout.b().layers[0][0]
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
    println!("{:?}", target_map);
    let parsed_chords = parse_input(&input_data);
    println!("{:?}", parsed_chords);
    let mapped_chords = map_to_physical_keys(parsed_chords, target_map);
    println!("{:?}", mapped_chords);
    let output = generate_defchordsv2_experimental(mapped_chords);
    fs::write("cfg_samples/chords.kbd", output).expect("Unable to write file");
}

fn parse_input(input: &str) -> Vec<ChordDefinition> {
    let re = Regex::new(r"^( ?[^\t]+)\t([^\t]+)(?:\t([^\t]*))?$").unwrap();
    input
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with("//"))
        .filter_map(|line| {
            re.captures(line).map(|caps| {
                let keys = caps
                    .get(1)
                    .unwrap()
                    .as_str()
                    .chars()
                    .map(|k| k.to_string())
                    .collect::<Vec<String>>();
                let action = caps
                    .get(2)
                    .unwrap()
                    .as_str()
                    .chars()
                    .map(|k| k.to_string())
                    .collect::<Vec<String>>();
                let comment = caps
                    .get(3)
                    .map_or(String::new(), |m| m.as_str().trim().to_string());
                ChordDefinition {
                    keys,
                    action,
                    comment,
                }
            })
        })
        .collect()
}

fn map_to_physical_keys(
    chords: Vec<ChordDefinition>,
    key_map: HashMap<String, String>,
) -> Vec<PhysicalChord> {
    let postprocess_map: HashMap<String, String> = [
        ("semicolon", ";"),
        ("slash", "/"),
        ("apostrophe", "'")
    ].iter().cloned().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let output_key_map: HashMap<String, String> = [
        ("y", "z"),
        ("z", "y"),
        (" ", "spc")
    ]
    .iter()
    .cloned()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    chords
        .into_iter()
        .map(|chord| {
            let keys = chord
                .keys
                .iter()
                .map(|key| key_map.get(key).unwrap_or(key).clone())
                .map(|key| postprocess_map.get(&key).unwrap_or(&key).clone())
                .collect::<Vec<String>>();
            let macro_text = chord
                .action
                .iter()
                .map(|key| if key.chars().next().map_or(false, |c| c.is_uppercase()) { "S-".to_string() + &key.to_lowercase() } else { key.clone() })          
                .map(|key| output_key_map.get(&key).unwrap_or(&key).clone())
                .collect::<Vec<String>>();
            PhysicalChord {
                keys,
                macro_text,
                comment: chord.comment,
            }
        })
        .collect()
}

fn generate_defchordsv2_experimental(chords: Vec<PhysicalChord>) -> String {
    let mut output = String::from("(defchordsv2-experimental\n");
    for chord in chords {
        let keys_str = chord.keys.join(" ");
        output.push_str(&format!(
            "  ({}) (macro {})     100 all-released () ;; {}\n",
            keys_str,
            chord.macro_text.join(" "),
            chord.comment
        ));
    }
    output.push_str(")\n");
    output
}

// Define necessary data structures

#[derive(Debug)]
struct ChordDefinition {
    keys: Vec<String>,
    action: Vec<String>,
    comment: String, // Optional comment field
}

#[derive(Debug)]
struct PhysicalChord {
    keys: Vec<String>,
    macro_text: Vec<String>,
    comment: String, // Optional comment field
}
