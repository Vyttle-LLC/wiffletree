//! `docs/host-protocol.md` lists every command with its class, as `Command` defines them.
use std::collections::BTreeMap;

fn snake(name: &str) -> String {
    let mut snake = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(c.to_ascii_lowercase());
    }
    snake
}

/// The `Self::` names in `source` from `start` up to `end`.
fn named(source: &str, start: &str, end: &str) -> Vec<String> {
    let from = source.find(start).unwrap();
    let to = from + source[from..].find(end).unwrap();
    source[from..to]
        .split("Self::")
        .skip(1)
        .map(|rest| rest.chars().take_while(|c| c.is_alphanumeric()).collect())
        .collect()
}

#[test]
fn the_protocol_doc_lists_every_command_with_its_class() {
    let source = include_str!("../src/lib.rs");
    let enum_start = source.find("pub enum Command {").unwrap();
    let enum_end = enum_start + source[enum_start..].find("\nimpl Command {").unwrap();
    let variants: Vec<&str> = source[enum_start..enum_end]
        .lines()
        .filter(|line| line.starts_with("    ") && !line.starts_with("     "))
        .map(str::trim)
        .filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
        .map(|line| line.trim_end_matches([',', '{', ' ']))
        .collect();
    let reads = named(source, "pub fn is_read(&self)", "=> true");
    let human = named(source, "pub fn is_human_only(&self)", "\n    }\n");
    let expected: BTreeMap<String, String> = variants
        .iter()
        .map(|variant| {
            let mut class = if reads.iter().any(|r| r == variant) {
                "read".to_string()
            } else {
                "change".to_string()
            };
            if human.iter().any(|h| h == variant) {
                class.push_str(", human only");
            }
            (snake(variant), class)
        })
        .collect();
    assert!(expected.len() > 80, "the enum was read: {}", expected.len());

    let doc = include_str!("../../../docs/host-protocol.md");
    let table = &doc[doc.find("## Commands").unwrap()..];
    let documented: BTreeMap<String, String> = table
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split(" | ").collect();
            let name = cells.first()?.strip_prefix("| `")?.strip_suffix('`')?;
            Some((name.to_string(), cells.get(2)?.to_string()))
        })
        .filter(|(name, _)| name != "type")
        .collect();
    assert_eq!(documented, expected);
}
