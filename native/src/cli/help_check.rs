//! The help cannot fall behind the commands: every flag a command's handler reads (in its
//! `cli/<group>.rs`, through the functions of that file it calls) appears in that command's
//! lines of `help.txt`, unless [`HIDDEN`] says why it is left out.
//!
//! Flags are read in `cli/` only (`args.text("k")`, `args.truthy("k")`, `args.get("k")`, a helper
//! called as `f(a, "k")` or `f(ctx, "k")`, a key constant of `flag_key` or of the file's own
//! `mod key`, or a `let f = |key: &str| ...` closure called as `f("k")`); the scan reads those forms.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use super::word::command;
use super::{command_help, Handler, INTERNAL};

/// A group file: its source and the commands it registers.
struct Group {
    name: &'static str,
    source: &'static str,
    commands: fn() -> Vec<(&'static str, Handler)>,
}

/// Every group file with commands of the help.
const GROUPS: [Group; 10] = [
    Group {
        name: "workflow.rs",
        source: include_str!("workflow.rs"),
        commands: super::workflow::commands,
    },
    Group {
        name: "comments.rs",
        source: include_str!("comments.rs"),
        commands: super::comments::commands,
    },
    Group {
        name: "design.rs",
        source: include_str!("design.rs"),
        commands: super::design::commands,
    },
    Group {
        name: "screen.rs",
        source: include_str!("screen.rs"),
        commands: super::screen::commands,
    },
    Group {
        name: "studio.rs",
        source: include_str!("studio.rs"),
        commands: super::studio::commands,
    },
    Group {
        name: "browser.rs",
        source: include_str!("browser.rs"),
        commands: super::browser::commands,
    },
    Group {
        name: "penpot.rs",
        source: include_str!("penpot.rs"),
        commands: super::penpot::commands,
    },
    Group {
        name: "tools.rs",
        source: include_str!("tools.rs"),
        commands: super::tools::commands,
    },
    Group {
        name: "telemetry.rs",
        source: include_str!("telemetry.rs"),
        commands: super::telemetry::commands,
    },
    Group {
        name: "hook.rs",
        source: include_str!("hook.rs"),
        commands: super::hook::commands,
    },
];

/// Flags every command takes, read in `main.rs` and `cli/mod.rs`, explained once in the help's
/// head rather than on each command line.
const GLOBAL: [&str; 3] = ["root", "help", "version"];

/// Flags a command reads but its help leaves out on purpose: (command, flag, why).
const HIDDEN: [(&str, &str, &str); 4] = [
    (
        command::SYSTEM,
        "only",
        "removed: `system init --only` refuses it and says what to do instead",
    ),
    (command::COMMENTS, "m", "the short form of --message"),
    (
        command::DECIDE,
        "text",
        "the decision as a flag instead of the trailing words",
    ),
    (command::SETUP, "playwright", "the old name of --browser"),
];

/// The source without its tests.
fn product_code(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

/// `pub const NAME: &str = "value";` of the block that starts with `start` (to its closing `}`).
fn constants(source: &str, start: &str) -> BTreeMap<String, String> {
    let Some(at) = source.find(start) else {
        return BTreeMap::new();
    };
    let block = &source[at..];
    let block = &block[..block.find("\n}").unwrap_or(block.len())];
    let re = Regex::new(r#"pub const ([A-Z_]+): &str = "([a-z][a-z0-9-]*)";"#).unwrap();
    re.captures_iter(block)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

/// The top-level functions of a file: name and body (to the next top-level item).
fn functions(source: &str) -> BTreeMap<String, String> {
    let re = Regex::new(r"(?m)^(?:pub(?:\([a-z]+\))? )?fn (\w+)").unwrap();
    let starts: Vec<(usize, String)> = re
        .captures_iter(source)
        .map(|c| (c.get(0).unwrap().start(), c[1].to_string()))
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(i, (at, name))| {
            let end = starts.get(i + 1).map_or(source.len(), |(next, _)| *next);
            let code: Vec<&str> = source[*at..end]
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            (name.clone(), code.join("\n"))
        })
        .collect()
}

/// Helpers of `workflow::state` that read a flag by key.
const STATE_HELPERS: [&str; 2] = ["arg_truthy", "arg_raw"];

/// A call that reads the flag `key` of the arguments: `a.text(key)`, or `helper(a, key)`.
fn reads_pattern(helpers: &BTreeSet<String>, key: &str) -> String {
    let names: Vec<String> = helpers.iter().map(|h| regex::escape(h)).collect();
    let mut forms = vec![format!(r"(?:\ba|args)\.(?:text|truthy|get)\(\s*{key}")];
    if !names.is_empty() {
        forms.push(format!(
            r"\b(?:{})\(\s*&?(?:a|args|ctx|ctx\.args),\s*{key}",
            names.join("|")
        ));
    }
    format!("(?:{})", forms.join("|"))
}

/// The functions of a file that read a flag by a `key: &str` parameter, with [`STATE_HELPERS`].
fn helpers(fns: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut found: BTreeSet<String> = STATE_HELPERS.iter().map(|h| h.to_string()).collect();
    loop {
        let reads = Regex::new(&reads_pattern(&found, r"key\b")).unwrap();
        let more: Vec<String> = fns
            .iter()
            .filter(|(name, body)| {
                !found.contains(*name)
                    && body.lines().next().is_some_and(|l| l.contains("key: &str"))
                    && reads.is_match(body)
            })
            .map(|(name, _)| name.clone())
            .collect();
        if more.is_empty() {
            return found;
        }
        found.extend(more);
    }
}

/// The flags one function body reads.
fn flags_read(
    body: &str,
    keys: &BTreeMap<String, String>,
    helpers: &BTreeSet<String>,
) -> BTreeSet<String> {
    const KEY: &str = r#""([a-z][a-z0-9-]*)""#;
    const NAMED: &str = r"(?:flag_key|key)::([A-Z_]+)";
    let mut found = BTreeSet::new();
    let literal = Regex::new(&reads_pattern(helpers, KEY)).unwrap();
    found.extend(literal.captures_iter(body).filter_map(|c| {
        c.iter()
            .skip(1)
            .flatten()
            .next()
            .map(|m| m.as_str().to_string())
    }));
    let named = Regex::new(&reads_pattern(helpers, NAMED)).unwrap();
    for c in named.captures_iter(body) {
        let name = c.iter().skip(1).flatten().next().unwrap().as_str();
        let key = keys
            .get(name)
            .unwrap_or_else(|| panic!("no flag key named {name}"));
        found.insert(key.clone());
    }
    // `let ids = |key: &str| flag(a, key) ...;` then `ids("screen")`.
    let reads_key = Regex::new(&reads_pattern(helpers, r"key\b")).unwrap();
    let closure = Regex::new(r"let (\w+) = \|key: &str\|([^\n]*)").unwrap();
    for c in closure.captures_iter(body) {
        if reads_key.is_match(&c[2]) {
            let call = Regex::new(&format!(r"\b{}\({KEY}\)", &c[1])).unwrap();
            found.extend(call.captures_iter(body).map(|k| k[1].to_string()));
        }
    }
    found
}

/// The flags a handler reads, through every function of its file it calls.
fn handler_flags(
    handler: &str,
    fns: &BTreeMap<String, String>,
    keys: &BTreeMap<String, String>,
    helpers: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![handler.to_string()];
    let mut flags = BTreeSet::new();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let body = &fns[&name];
        flags.extend(flags_read(body, keys, helpers));
        for callee in fns.keys() {
            if callee != &name
                && Regex::new(&format!(r"\b{callee}\("))
                    .unwrap()
                    .is_match(body)
            {
                todo.push(callee.clone());
            }
        }
    }
    flags
}

/// Whether the help text names `--flag` (not as the start of a longer flag).
fn names_flag(help: &str, flag: &str) -> bool {
    Regex::new(&format!(r"--{}(?:[^a-z0-9-]|$)", regex::escape(flag)))
        .unwrap()
        .is_match(help)
}

/// Every flag a command reads is in its help, or in [`HIDDEN`] with a reason.
pub fn missing_from_help() -> Vec<String> {
    let flag_keys = constants(include_str!("mod.rs"), "pub(crate) mod flag_key {");
    let table = Regex::new(r"\(command::[A-Z_]+,\s*(\w+) as Handler\)").unwrap();
    let mut missing = Vec::new();
    for group in &GROUPS {
        let source = product_code(group.source);
        let mut keys = flag_keys.clone();
        keys.extend(constants(source, "mod key {"));
        let fns = functions(source);
        let helpers = helpers(&fns);
        let handlers: Vec<String> = table
            .captures_iter(source)
            .map(|c| c[1].to_string())
            .collect();
        let words = (group.commands)();
        assert_eq!(
            handlers.len(),
            words.len(),
            "{}: the scan did not find every handler of the table",
            group.name
        );
        for ((word, _), handler) in words.iter().zip(&handlers) {
            if INTERNAL.contains(word) {
                continue;
            }
            // Commands that share a handler share its flags (`lint` and `check`): a flag is in
            // the help when the lines of one of them name it.
            let help: String = words
                .iter()
                .zip(&handlers)
                .filter(|(_, h)| *h == handler)
                .filter_map(|((w, _), _)| command_help(w))
                .collect();
            for flag in handler_flags(handler, &fns, &keys, &helpers) {
                let hidden = HIDDEN.iter().any(|(c, f, _)| c == word && *f == flag);
                if !GLOBAL.contains(&flag.as_str()) && !hidden && !names_flag(&help, &flag) {
                    missing.push(format!("dfw {word} --{flag} ({})", group.name));
                }
            }
        }
    }
    missing
}

#[test]
fn every_flag_a_command_reads_is_in_its_help() {
    let missing = missing_from_help();
    assert!(
        missing.is_empty(),
        "flags missing from help.txt:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_scan_finds_the_forms_flags_are_read_in() {
    let keys = BTreeMap::from([("PAGE".to_string(), "page".to_string())]);
    // `get` spelt apart: the source scan of JSON keys (`tests/conventions.rs`) reads `.get("`.
    let get = "get";
    let body = format!(
        r#"
        a.truthy("one"); ctx.args.text("two"); args.{get}("three");
        flag(a, "four"); arg_truthy(&ctx.args, "five"); text(ctx, key::PAGE);
        let ids = |key: &str| flag(a, key); ids("six");
        out("not-a-flag"); c.{get}("field"); sub(ctx, "list");
        let list = |key: &str| r[key].clone(); list("matches");
    "#
    );
    let helpers = BTreeSet::from([
        "flag".to_string(),
        "arg_truthy".to_string(),
        "text".to_string(),
    ]);
    let found: Vec<String> = flags_read(&body, &keys, &helpers).into_iter().collect();
    assert_eq!(
        found,
        ["five", "four", "one", "page", "six", "three", "two"]
    );
    assert!(names_flag("[--no-a11y] x", "no-a11y") && !names_flag("[--no-a11y]", "no"));
}

/// Every subcommand `dfw comments` answers (a `"word" =>` arm of its handler) has a help line.
#[test]
fn every_comments_subcommand_is_in_its_help() {
    let source = product_code(include_str!("comments.rs"));
    let arm = Regex::new(r#"(?m)^\s+"([a-z-]+)" =>"#).unwrap();
    let help = command_help(command::COMMENTS).unwrap_or_default();
    let missing: Vec<String> = arm
        .captures_iter(source)
        .map(|c| c[1].to_string())
        .filter(|word| !help.contains(&format!("dfw {} {word}", command::COMMENTS)))
        .collect();
    assert!(
        missing.is_empty(),
        "dfw comments subcommands missing from help.txt: {missing:?}"
    );
}
