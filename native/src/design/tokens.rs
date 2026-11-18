//! Ports `core/lib/tokens.mjs`: DTCG tokens to CSS declarations and `tokens.css`.

use crate::core::js;
use crate::design::edit::collect_tokens;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::to_json_compact;

/// `cssVarName`: `color.accent` is `--color-accent`.
pub fn css_var_name(name: &str) -> String {
    format!("--{}", name.replace('.', "-"))
}

/// `path.relative(from, to)` for two paths (made absolute first).
pub fn relative_path(from: &Path, to: &Path) -> String {
    let (from, to) = (resolve(from), resolve(to));
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join(MAIN_SEPARATOR_STR)
}

/// An error where the Node code threw a `TypeError` on a malformed token value.
fn malformed(what: &str) -> DfwError {
    DfwError::internal(format!("Malformed token value: {what}."))
}

struct Token {
    name: String,
    value: Value,
    ty: Option<String>,
}

/// Every token of the tree by dotted name, with the `$type` it has or inherits: the walk of
/// [`collect_tokens`] kept as a `Map` (a repeated name keeps its first place and takes the
/// later token). `node.$type || type`: a truthy type that is not a string matches no type.
fn collect(tree: &Value) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for t in collect_tokens(tree) {
        let token = Token {
            ty: t.ty.map(|ty| ty.as_str().unwrap_or_default().to_string()),
            name: t.name,
            value: t.value,
        };
        match out.iter_mut().find(|o| o.name == token.name) {
            Some(slot) => *slot = token,
            None => out.push(token),
        }
    }
    out
}

/// `fmtNumber`: integers as they are, anything else rounded to four decimals.
fn fmt_number(value: Option<&Value>) -> String {
    fmt_f64(js::to_number(value))
}

fn fmt_f64(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 {
        js::number(n)
    } else {
        js::number(js::round(n * 10000.0) / 10000.0)
    }
}

fn alpha_suffix(alpha: Option<&Value>) -> String {
    // `a !== 1`: only the number one is left out.
    match alpha {
        Some(Value::Number(n)) if n.as_f64() == Some(1.0) => String::new(),
        None => String::new(),
        other => format!(" / {}", js::to_string(other)),
    }
}

fn color_to_css(value: Option<&Value>) -> Result<String, DfwError> {
    let fields = match value {
        Some(Value::String(s)) => return Ok(s.clone()),
        Some(Value::Object(map)) => Some(map),
        Some(Value::Array(_)) => None,
        other => {
            let shown = other.map_or("undefined".to_string(), to_json_compact);
            return Err(DfwError::internal(format!(
                "Unsupported color value {shown}"
            )));
        }
    };
    let get = |key: &str| fields.and_then(|m| m.get(key));
    let alpha = js::present(get("alpha"));
    let opaque = alpha_suffix(alpha).is_empty();
    let components: &[Value] = match get("components").filter(|c| js::truthy(Some(c))) {
        None => &[],
        Some(Value::Array(items)) => items,
        Some(_) => return Err(malformed("color components must be a list")),
    };
    let none = |v: &Value| v.as_str() == Some("none");
    let numbers = |scale: f64| -> String {
        let parts: Vec<String> = components
            .iter()
            .map(|c| {
                if none(c) {
                    "none".into()
                } else {
                    fmt_f64(js::to_number(Some(c)) * scale)
                }
            })
            .collect();
        parts.join(" ")
    };
    let plain = || -> String {
        let parts: Vec<String> = components.iter().map(|c| fmt_number(Some(c))).collect();
        parts.join(" ")
    };
    let hex = get("hex").filter(|h| js::truthy(Some(h)));
    let space = get("colorSpace");
    let a = alpha_suffix(alpha);
    Ok(match space.and_then(Value::as_str) {
        Some("srgb") => match hex {
            Some(hex) if opaque => js::to_string(Some(hex)),
            _ => format!("rgb({}{a})", numbers(255.0)),
        },
        Some("oklch") => {
            let third = match components.get(2) {
                Some(c) if none(c) => "none".to_string(),
                c => fmt_number(c),
            };
            format!(
                "oklch({} {} {third}{a})",
                fmt_number(components.first()),
                fmt_number(components.get(1))
            )
        }
        Some(name @ ("oklab" | "lab" | "lch" | "hsl" | "hwb")) => format!("{name}({}{a})", plain()),
        _ => match hex {
            Some(hex) => js::to_string(Some(hex)),
            None => format!("color({} {}{a})", js::to_string(space), plain()),
        },
    })
}

/// `{color.accent}` references in a string value become `var(--color-accent)`.
fn aliases_to_css(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let body = &rest[open + 1..];
        let stop = body.find(['{', '}']);
        match stop {
            Some(len) if len > 0 && body[len..].starts_with('}') => {
                out.push_str(&rest[..open]);
                out.push_str(&format!("var({})", css_var_name(&body[..len])));
                rest = &body[len + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}

fn field<'a>(
    value: Option<&'a Value>,
    key: &str,
    what: &str,
) -> Result<Option<&'a Value>, DfwError> {
    match value {
        Some(Value::Object(map)) => Ok(map.get(key)),
        Some(Value::Null) | None => Err(malformed(what)),
        Some(_) => Ok(None),
    }
}

fn list<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a [Value], DfwError> {
    match value {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(malformed(what)),
    }
}

// One match over the DTCG types, as the switch of tokens.mjs (over 120 lines once formatted).
#[allow(clippy::too_many_lines)]
fn value_to_css(value: Option<&Value>, ty: &str) -> Result<String, DfwError> {
    if let Some(Value::String(text)) = value {
        return Ok(aliases_to_css(text));
    }
    let zero = |unit: &str| serde_json::json!({ "value": 0, "unit": unit });
    Ok(match ty {
        "color" => color_to_css(value)?,
        "dimension" | "duration" => match value {
            Some(Value::Object(map)) if map.contains_key("value") => {
                format!(
                    "{}{}",
                    fmt_number(map.get("value")),
                    js::to_string(map.get("unit"))
                )
            }
            Some(Value::Null) => return Err(malformed("a dimension is null")),
            other => js::to_string(other),
        },
        "number" | "fontWeight" => js::to_string(value),
        "fontFamily" => {
            let single = [value.cloned().unwrap_or(Value::Null)];
            let families = match value {
                Some(Value::Array(items)) => items.as_slice(),
                _ => &single,
            };
            let parts: Vec<String> = families
                .iter()
                .map(|f| {
                    let name = js::to_string(Some(f));
                    if name.contains(|c: char| crate::core::js::is_space(c) || c == ',') {
                        format!("\"{name}\"")
                    } else if f.is_null() {
                        String::new()
                    } else {
                        name
                    }
                })
                .collect();
            parts.join(", ")
        }
        "cubicBezier" => {
            let parts: Vec<String> = list(value, "a cubicBezier must be a list")?
                .iter()
                .map(|n| fmt_number(Some(n)))
                .collect();
            format!("cubic-bezier({})", parts.join(", "))
        }
        "shadow" => {
            // Blur and spread are optional in CSS; a missing one must not print "undefined".
            let len = |v: Option<&Value>| {
                value_to_css(Some(js::present(v).unwrap_or(&zero("px"))), "dimension")
            };
            let one = |shadow: &Value| -> Result<String, DfwError> {
                let get = |key: &str| field(Some(shadow), key, "a shadow layer is null");
                let parts = [
                    if js::truthy(get("inset")?) {
                        "inset".to_string()
                    } else {
                        String::new()
                    },
                    len(get("offsetX")?)?,
                    len(get("offsetY")?)?,
                    len(get("blur")?)?,
                    match js::present(get("spread")?) {
                        Some(spread) => len(Some(spread))?,
                        None => String::new(),
                    },
                    value_to_css(get("color")?, "color")?,
                ];
                Ok(parts
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "))
            };
            match value {
                Some(Value::Array(layers)) => layers
                    .iter()
                    .map(one)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", "),
                Some(single) => one(single)?,
                None => return Err(malformed("a shadow has no value")),
            }
        }
        "border" => {
            let get = |key: &str| field(value, key, "a border is null");
            format!(
                "{} {} {}",
                value_to_css(get("width")?, "dimension")?,
                js::to_string(get("style")?),
                value_to_css(get("color")?, "color")?
            )
        }
        "transition" => {
            let get = |key: &str| field(value, key, "a transition is null");
            let delay = zero("ms");
            format!(
                "{} {} {}",
                value_to_css(get("duration")?, "duration")?,
                value_to_css(get("timingFunction")?, "cubicBezier")?,
                value_to_css(
                    Some(js::present(get("delay")?).unwrap_or(&delay)),
                    "duration"
                )?
            )
        }
        // A string was returned above; every other shape is "dashed".
        "strokeStyle" => "dashed".to_string(),
        "gradient" => {
            let stops = list(value, "a gradient must be a list of stops")?;
            let parts = stops
                .iter()
                .map(|stop| {
                    let get = |key: &str| field(Some(stop), key, "a gradient stop is null");
                    let position = js::to_number(get("position")?) * 100.0;
                    Ok(format!(
                        "{} {}%",
                        value_to_css(get("color")?, "color")?,
                        fmt_f64(position)
                    ))
                })
                .collect::<Result<Vec<_>, DfwError>>()?;
            parts.join(", ")
        }
        _ => match value {
            Some(Value::Number(n)) => js::number(n.as_f64().unwrap_or(f64::NAN)),
            Some(other) => to_json_compact(other),
            None => "undefined".to_string(),
        },
    })
}

/// `tokensToDeclarations`: `(css variable, css value)` pairs in the order JavaScript walks the
/// tree; a typography token expands to one variable per sub-property.
pub fn tokens_to_declarations(tree: &Value) -> Result<Vec<(String, String)>, DfwError> {
    const TYPOGRAPHY: [(&str, &str, &str); 5] = [
        ("fontFamily", "font-family", "fontFamily"),
        ("fontSize", "font-size", "dimension"),
        ("fontWeight", "font-weight", "fontWeight"),
        ("letterSpacing", "letter-spacing", "dimension"),
        ("lineHeight", "line-height", "number"),
    ];
    let mut decls = Vec::new();
    for token in collect(tree) {
        if token.value.is_null() {
            return Err(DfwError::new(format!(
                "Token \"{}\" has no value ($value is null).",
                token.name
            )));
        }
        let ty = token.ty.as_deref().unwrap_or_default();
        let var = css_var_name(&token.name);
        if let (true, Value::Object(parts)) = (ty == "typography", &token.value) {
            for (key, suffix, part_type) in TYPOGRAPHY {
                if let Some(part) = parts.get(key) {
                    decls.push((
                        format!("{var}-{suffix}"),
                        value_to_css(Some(part), part_type)?,
                    ));
                }
            }
            continue;
        }
        decls.push((var, value_to_css(Some(&token.value), ty)?));
    }
    Ok(decls)
}

/// What [`build_tokens`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltTokens {
    /// Path of `design/tokens/tokens.css`.
    pub out: PathBuf,
    /// Names of the themes found in `design/tokens/themes/`.
    pub themes: Vec<String>,
}

/// `.json` file names of a directory in the order Node lists them (sorted by bytes), without
/// the extension.
pub fn json_names(dir: &Path) -> Result<Vec<String>, DfwError> {
    Ok(dir_names(dir)?
        .into_iter()
        .filter_map(|f| f.strip_suffix(".json").map(str::to_string))
        .collect())
}

/// `fs.readdirSync(dir)`: the entry names, sorted by bytes as libuv returns them. A missing
/// directory is an empty list here; callers check `exists()` where Node did.
pub fn dir_names(dir: &Path) -> Result<Vec<String>, DfwError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(DfwError::internal(format!("{}: {err}", dir.display()))),
    };
    let mut names = Vec::new();
    for entry in entries {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(names)
}

/// `buildTokens`: write `design/tokens/tokens.css` from `tokens.json` and one
/// `[data-theme="<name>"]` block per `design/tokens/themes/<name>.json`.
pub fn build_tokens(root: &Path) -> Result<BuiltTokens, DfwError> {
    let dir = project_paths(root).tokens;
    let base = dir.join("tokens.json");
    if !base.exists() {
        return Err(DfwError::new("design/tokens/tokens.json not found."));
    }
    let parse = |file: &Path| -> Result<Value, DfwError> {
        let text = js::read_text(file)?;
        serde_json::from_str(&text)
            .map_err(|err| DfwError::new(format!("{}: {err}", relative_path(root, file))))
    };
    let block = |selector: &str, tree: &Value| -> Result<String, DfwError> {
        let lines: Vec<String> = tokens_to_declarations(tree)?
            .iter()
            .map(|(k, v)| format!("  {k}: {v};"))
            .collect();
        Ok(format!("{selector} {{\n{}\n}}\n", lines.join("\n")))
    };
    let mut css = String::from("/* Generated by \"dfw tokens build\" from design/tokens/*.json (W3C DTCG). Do not edit by hand. */\n");
    css.push_str(&block(":root", &parse(&base)?)?);
    let themes_dir = dir.join("themes");
    let themes = json_names(&themes_dir)?;
    for name in &themes {
        css.push('\n');
        css.push_str(&block(
            &format!("[data-theme=\"{name}\"]"),
            &parse(&themes_dir.join(format!("{name}.json")))?,
        )?);
    }
    let out = dir.join("tokens.css");
    fs::write(&out, css)?;
    Ok(BuiltTokens { out, themes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use serde_json::json;

    fn decls(tree: Value) -> Vec<(String, String)> {
        tokens_to_declarations(&tree).unwrap()
    }

    fn value_of(tree: Value, var: &str) -> String {
        decls(tree).into_iter().find(|(k, _)| k == var).unwrap().1
    }

    /// tokens: DTCG 2025.10 values and aliases (tests/unit/dfw.test.mjs; the lintTokens
    /// assertions belong to design/lint)
    #[test]
    fn dtcg_values_and_aliases() {
        let tree = json!({
            "color": {
                "$type": "color",
                "blue": { "$value": { "colorSpace": "srgb", "components": [0, 0.4, 1], "hex": "#0066ff" } },
                "accent": { "$value": "{color.blue}" },
                "soft": { "$value": { "colorSpace": "oklch", "components": [0.9, 0.05, 250], "alpha": 0.5 } },
            },
            "space": { "$type": "dimension", "4": { "$value": { "value": 16, "unit": "px" } } },
            "font": { "body": { "$type": "fontFamily", "$value": ["Source Sans 3", "sans-serif"] } },
            "shadow": { "card": { "$type": "shadow", "$value": { "color": "{color.blue}", "offsetX": { "value": 0, "unit": "px" }, "offsetY": { "value": 1, "unit": "px" }, "blur": { "value": 2, "unit": "px" }, "spread": { "value": 0, "unit": "px" } } } },
        });
        let d: std::collections::HashMap<String, String> = decls(tree).into_iter().collect();
        assert_eq!(d["--color-blue"], "#0066ff");
        assert_eq!(d["--color-accent"], "var(--color-blue)");
        assert_eq!(d["--color-soft"], "oklch(0.9 0.05 250 / 0.5)");
        assert_eq!(d["--space-4"], "16px");
        assert_eq!(d["--font-body"], "\"Source Sans 3\", sans-serif");
        assert_eq!(d["--shadow-card"], "0px 1px 2px 0px var(--color-blue)");
    }

    /// tokens: a shadow without blur/spread prints valid CSS; a null $value is a clear error
    /// (tests/unit/audit-fixes.test.mjs)
    #[test]
    fn shadow_without_blur_or_spread_and_null_value() {
        let px = |value: i32| json!({ "value": value, "unit": "px" });
        let decl = |shadow: Value| {
            value_of(
                json!({ "shadow": { "$type": "shadow", "sm": { "$value": shadow } } }),
                "--shadow-sm",
            )
        };
        assert_eq!(
            decl(json!({ "offsetX": px(0), "offsetY": px(1), "blur": px(2), "color": "#000000" })),
            "0px 1px 2px #000000"
        );
        assert_eq!(
            decl(
                json!({ "offsetX": px(0), "offsetY": px(1), "spread": px(3), "color": "#000000" })
            ),
            "0px 1px 0px 3px #000000"
        );
        let err = tokens_to_declarations(
            &json!({ "space": { "$type": "dimension", "a": { "$value": null } } }),
        )
        .unwrap_err();
        assert!(
            err.message().contains("Token \"space.a\" has no value"),
            "{err}"
        );
        assert_eq!(err.exit_code(), 1);
    }

    /// penpot tokens: a math expression keeps its aliases (the tokensToDeclarations assertion
    /// of tests/unit/penpot-tokens.test.mjs)
    #[test]
    fn aliases_inside_an_expression() {
        let tree =
            json!({ "space": { "8": { "$type": "dimension", "$value": "calc({space.4} * 2)" } } });
        assert_eq!(value_of(tree, "--space-8"), "calc(var(--space-4) * 2)");
        assert_eq!(
            aliases_to_css("{a.b}{c}{}{d{e.f}} {"),
            "var(--a-b)var(--c){}{dvar(--e-f)} {"
        );
    }

    #[test]
    fn declarations_follow_javascript_key_order_and_number_format() {
        let tree: Value = serde_json::from_str(
            r##"{"space":{"$type":"dimension","half":{"$value":{"value":2,"unit":"px"}},"2":{"$value":{"value":8.0,"unit":"px"}},"1":{"$value":{"value":4.123456,"unit":"px"}}},
                "n":{"$type":"number","$value":1.50},"w":{"$type":"fontWeight","$value":600},
                "ease":{"$type":"cubicBezier","$value":[0.2,0,0,1]},
                "c":{"$type":"color","a":{"$value":{"colorSpace":"srgb","components":[1,0.5,"none"],"alpha":0.5}},"b":{"$value":{"colorSpace":"display-p3","components":[1,0,0]}},"h":{"$value":{"colorSpace":"hsl","components":[210,50,40]}}},
                "t":{"$type":"typography","$value":{"fontFamily":["Inter","sans-serif"],"fontSize":{"value":16,"unit":"px"},"fontWeight":600,"lineHeight":1.4}},
                "b":{"$type":"border","$value":{"width":{"value":1,"unit":"px"},"style":"solid","color":"{c.a}"}},
                "tr":{"$type":"transition","$value":{"duration":{"value":120,"unit":"ms"},"timingFunction":[0,0,1,1]}},
                "g":{"$type":"gradient","$value":[{"color":"#000","position":0},{"color":"#fff","position":0.5}]},
                "s":{"$type":"string","$value":true},"list":[{"$type":"number","$value":3}]}"##,
        )
        .unwrap();
        let got: Vec<String> = decls(tree)
            .into_iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        assert_eq!(
            got,
            [
                "--space-1: 4.1235px",
                "--space-2: 8px",
                "--space-half: 2px",
                "--n: 1.5",
                "--w: 600",
                "--ease: cubic-bezier(0.2, 0, 0, 1)",
                "--c-a: rgb(255 127.5 none / 0.5)",
                "--c-b: color(display-p3 1 0 0)",
                "--c-h: hsl(210 50 40)",
                "--t-font-family: Inter, sans-serif",
                "--t-font-size: 16px",
                "--t-font-weight: 600",
                "--t-line-height: 1.4",
                "--b: 1px solid var(--c-a)",
                "--tr: 120ms cubic-bezier(0, 0, 1, 1) 0ms",
                "--g: #000 0%, #fff 50%",
                "--s: true",
                "--list-0: 3",
            ]
        );
    }

    #[test]
    fn build_tokens_writes_root_and_theme_blocks() {
        let root = test_dir("tokens");
        let dir = root.join("design/tokens");
        assert!(build_tokens(&root)
            .unwrap_err()
            .message()
            .contains("tokens.json not found"));
        fs::create_dir_all(dir.join("themes")).unwrap();
        fs::write(
            dir.join("tokens.json"),
            r##"{"color":{"$type":"color","ink":{"$value":"#111111"}}}"##,
        )
        .unwrap();
        fs::write(
            dir.join("themes/dark.json"),
            r##"{"color":{"$type":"color","ink":{"$value":"#fafafa"}}}"##,
        )
        .unwrap();
        fs::write(dir.join("themes/notes.txt"), "x").unwrap();
        let built = build_tokens(&root).unwrap();
        assert_eq!(built.themes, ["dark"]);
        assert_eq!(
            fs::read_to_string(&built.out).unwrap(),
            "/* Generated by \"dfw tokens build\" from design/tokens/*.json (W3C DTCG). Do not edit by hand. */\n:root {\n  --color-ink: #111111;\n}\n\n[data-theme=\"dark\"] {\n  --color-ink: #fafafa;\n}\n"
        );
        fs::write(dir.join("themes/dark.json"), "{").unwrap();
        let err = build_tokens(&root).unwrap_err();
        assert!(
            err.message()
                .starts_with(&relative_path(&root, &dir.join("themes/dark.json"))),
            "{err}"
        );
        assert_eq!(
            relative_path(Path::new("/a/b/c"), Path::new("/a/d/e.txt")),
            ["..", "..", "d", "e.txt"].join(MAIN_SEPARATOR_STR)
        );
    }
}
