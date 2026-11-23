//! The job list of `dfw shoot` (`cmdShoot` of `core/lib/cli.mjs`): which pages are shot at
//! which size into which file. Screenshots of any URL (`--url`), or of the design served by
//! the running Studio: screens x variants x breakpoints x states x themes, and the design
//! system page. The shots themselves are taken by [`super::screenshot`].

use std::path::Path;

use serde_json::{json, Value};

use super::screenshot::Job;
use crate::core::config::{load_config, studio_breakpoints, Breakpoint};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, project_paths, rel, resolve, ProjectPaths};
use crate::core::process::is_alive;
use crate::core::util::read_json;
use crate::design::manifest::{self, key};
use crate::workflow::state::{load_state, state_field_error};

/// What to shoot, as the flags of `dfw shoot` give it. A text flag given alone reads `"true"`,
/// as in cli.mjs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShootOptions {
    /// `--out <file>` for a URL: `{bp}` is the breakpoint.
    pub out: Option<String>,
    /// `--viewport`: the visible part only, not the whole page.
    pub viewport: bool,
    /// `--selector <css>`: one element.
    pub selector: Option<String>,
    /// `--system`: the design system page.
    pub system: bool,
    /// `--screen <id>`, `--variant <id>`, `--page <id>`: only these.
    pub screen: Option<String>,
    pub variant: Option<String>,
    pub page: Option<String>,
    /// `--states`: every state of a screen (and the default); else `--state <name>`.
    pub states: bool,
    pub state: Option<String>,
    /// `--themes`: every theme; else `--theme <name>`.
    pub themes: bool,
    pub theme: Option<String>,
}

/// `new URL(url).pathname` for the default screenshot name. Not the whole WHATWG parser: dot
/// segments and percent-encoding are left as written.
fn url_pathname(url: &str) -> Result<String, DfwError> {
    let invalid = || DfwError::internal("Invalid URL");
    let colon = url.find(':').ok_or_else(invalid)?;
    let scheme = url[..colon].to_ascii_lowercase();
    let well_formed = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !well_formed {
        return Err(invalid());
    }
    let rest = &url[colon + 1..];
    let path_end = |s: &str| s.find(['?', '#']).unwrap_or(s.len());
    let special = ["http", "https", "ws", "wss", "ftp", "file"].contains(&scheme.as_str());
    if !special && !rest.starts_with("//") {
        return Ok(rest[..path_end(rest)].to_string());
    }
    // The authority follows "//"; a special scheme written with fewer slashes still has one.
    let after = rest
        .strip_prefix("//")
        .unwrap_or_else(|| rest.trim_start_matches('/'));
    let authority_end = after.find(['/', '\\', '?', '#']).unwrap_or(after.len());
    if special && scheme != "file" && authority_end == 0 {
        return Err(invalid());
    }
    let path = &after[authority_end..];
    let path = &path[..path_end(path)];
    Ok(if path.is_empty() && special {
        "/".to_string()
    } else {
        path.to_string()
    })
}

/// `str.replace(/\W+/g, '_')` (or another class of kept characters).
fn squash(text: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if keep(c) {
            out.push(c);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `new URLSearchParams()` with `theme` and `state` set, as text.
fn query(theme: Option<&str>, state: Option<&str>) -> String {
    let encode = |v: &str| {
        let mut out = String::new();
        for b in v.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                    out.push(char::from(b))
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    };
    let pairs: Vec<String> = [("theme", theme), ("state", state)]
        .iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}={}", encode(v))))
        .collect();
    pairs.join("&")
}

/// The breakpoints to shoot: those of design/design.json, else of the config, kept when `bp`
/// (`--bp a,b`) names them.
pub fn shoot_breakpoints(root: &Path, bp: Option<&str>) -> Result<Vec<Breakpoint>, DfwError> {
    let config = load_config(Some(root))?;
    let manifest = read_json(&project_paths(root).manifest, Some(json!({})))?;
    let all = match manifest.get(key::BREAKPOINTS).filter(|b| js::truthy(b)) {
        Some(list) => list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Breakpoint::from_value)
            .collect(),
        None => studio_breakpoints(&config),
    };
    Ok(match bp {
        Some(ids) => {
            let ids: Vec<&str> = ids.split(',').collect();
            all.into_iter()
                .filter(|b| ids.contains(&b.id.as_str()))
                .collect()
        }
        None => all,
    })
}

/// Screenshots of any URL: `--url <u> [--out <file>]`, one per breakpoint.
pub fn url_jobs(
    root: &Path,
    url: &str,
    bps: &[Breakpoint],
    opts: &ShootOptions,
) -> Result<Vec<Job>, DfwError> {
    let mut jobs = Vec::new();
    for bp in bps {
        let out_file = match opts.out.as_deref() {
            Some(name) => {
                // Several breakpoints into one --out name would overwrite each other: suffix
                // the breakpoint.
                let name = if !name.contains("{bp}") && bps.len() > 1 {
                    // name.replace(/(\.\w+)?$/, (ext) => `__{bp}${ext || '.png'}`)
                    let ext_at = name
                        .rfind('.')
                        .filter(|at| name.len() > at + 1 && name[at + 1..].chars().all(word));
                    match ext_at {
                        Some(at) => format!("{}__{{bp}}{}", &name[..at], &name[at..]),
                        None => format!("{name}__{{bp}}.png"),
                    }
                } else {
                    name.to_string()
                };
                resolve(name.replacen("{bp}", &bp.id, 1))
            }
            None => {
                let page = squash(&url_pathname(url)?, word);
                let page = if page.is_empty() { "root" } else { &page };
                project_paths(root)
                    .shots
                    .join(file::SHOTS_URL)
                    .join(format!("{page}__{}.png", bp.id))
            }
        };
        jobs.push(Job {
            url: url.to_string(),
            width: bp.width,
            height: bp.height,
            out: out_file,
            full_page: !opts.viewport,
            selector: opts.selector.clone(),
            scale: 1.0,
            // A file the user named with --out is theirs to place; the default is in the project.
            inside: opts.out.is_none().then(|| root.to_path_buf()),
        });
    }
    Ok(jobs)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect()
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The folder of this design version's shots: `v<design.version>`, `v1` without a state.
fn shots_version(root: &Path) -> Result<String, DfwError> {
    Ok(match load_state(root)? {
        Some(state) => format!(
            "v{}",
            js::to_string(
                state
                    .get("design")
                    .ok_or_else(|| state_field_error("design", "an object"))?
                    .get("version")
            )
        ),
        None => "v1".to_string(),
    })
}

/// The screens of page `page`: those its entry in `pages` lists, then those that name it.
fn screens_of_page(m: &Value, screens: &[&Value], page: &str) -> Vec<String> {
    let listed = manifest::items(m.get(key::PAGES))
        .iter()
        .find(|pg| text(pg, "id") == page)
        .map(|pg| strings(pg.get(key::SCREENS)))
        .unwrap_or_default();
    let tagged = screens
        .iter()
        .filter(|s| text(s, "page") == page)
        .map(|s| text(s, "id").to_string());
    listed.into_iter().chain(tagged).collect()
}

/// The states and themes a screen is shot in (`None` is the default of each).
fn states_and_themes(
    m: &Value,
    screen: &Value,
    opts: &ShootOptions,
) -> (Vec<Option<String>>, Vec<Option<String>>) {
    let states = if opts.states {
        let own = screen
            .get("states")
            .filter(|v| js::truthy(v))
            .or(m.get("states"));
        std::iter::once(None)
            .chain(strings(own).into_iter().map(Some))
            .collect()
    } else {
        vec![opts.state.clone()]
    };
    let themes = if opts.themes {
        let all = strings(m.get("themes"));
        if all.is_empty() {
            vec![None]
        } else {
            all.into_iter().map(Some).collect()
        }
    } else {
        vec![opts.theme.clone()]
    };
    (states, themes)
}

/// The Studio's URL when it runs, and the manifest; the errors `cmdShoot` gives without them.
fn running_studio(p: &ProjectPaths) -> Result<(Value, Value), DfwError> {
    let info = read_json(&p.studio, Some(Value::Null))?;
    if !js::truthy(&info) || !is_alive(info.get("pid")) {
        return Err(DfwError::new(
            "Start the studio first (dfw studio start) or pass --url.",
        ));
    }
    let m = read_json(&p.manifest, Some(Value::Null))?;
    if !js::truthy(&m) {
        return Err(DfwError::new(format!("{} not found.", rel::MANIFEST)));
    }
    Ok((info, m))
}

/// Screenshots of the design, served by the running Studio: screens x variants x breakpoints
/// x states x themes, and the design system page with `--system`.
pub fn studio_jobs(
    root: &Path,
    bps: &[Breakpoint],
    opts: &ShootOptions,
) -> Result<Vec<Job>, DfwError> {
    let p = project_paths(root);
    let (info, m) = running_studio(&p)?;
    let version = shots_version(root)?;
    let screens_all: Vec<&Value> = manifest::items(m.get(key::SCREENS)).iter().collect();
    let page_screens: Option<Vec<String>> = opts
        .page
        .as_deref()
        .map(|page| screens_of_page(&m, &screens_all, page));
    let studio_url = text(&info, "url");
    let shots_dir = p.shots.join(version);
    let name = |parts: &[&str]| {
        let kept: Vec<&str> = parts.iter().copied().filter(|s| !s.is_empty()).collect();
        format!("{}.png", kept.join("__"))
    };
    let (screen, variant, selector) = (&opts.screen, &opts.variant, &opts.selector);
    let mut jobs = Vec::new();
    if opts.system {
        if let Some(bp) = bps.first() {
            jobs.push(Job {
                url: format!("{studio_url}__dfw/system.html"),
                width: bp.width,
                height: bp.height,
                out: shots_dir.join(name(&["_system", &bp.id])),
                full_page: true,
                selector: None,
                scale: 1.0,
                inside: Some(root.to_path_buf()),
            });
        }
    }
    let screens = if opts.system && screen.is_none() && page_screens.is_none() {
        Vec::new()
    } else {
        screens_all
    };
    let element = selector.as_ref().map(|s| {
        let cleaned = squash(s, |c| word(c) || c == '.' || c == '-');
        let cleaned: String = cleaned.trim_matches('_').chars().take(60).collect();
        format!("el-{cleaned}")
    });
    for s in screens {
        let id = text(s, "id");
        if screen.as_deref().is_some_and(|only| only != id) {
            continue;
        }
        if page_screens
            .as_ref()
            .is_some_and(|ids| !ids.iter().any(|x| x == id))
        {
            continue;
        }
        let (states, themes) = states_and_themes(&m, s, opts);
        for v in manifest::items(s.get(key::VARIANTS)).iter() {
            if variant.as_deref().is_some_and(|only| only != text(v, "id")) {
                continue;
            }
            for bp in bps {
                for st in &states {
                    for th in &themes {
                        let q = query(th.as_deref(), st.as_deref());
                        let q = if q.is_empty() { q } else { format!("?{q}") };
                        jobs.push(Job {
                            url: format!("{studio_url}{}{q}", rel::design(text(v, "file"))),
                            width: bp.width,
                            height: bp.height,
                            out: shots_dir.join(name(&[
                                &id.replace('/', "__"),
                                text(v, "id"),
                                &bp.id,
                                th.as_deref().unwrap_or(""),
                                st.as_deref().unwrap_or(""),
                                element.as_deref().unwrap_or(""),
                            ])),
                            full_page: !opts.viewport,
                            selector: selector.clone(),
                            scale: 1.0,
                            inside: Some(root.to_path_buf()),
                        });
                    }
                }
            }
        }
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::screen_page;
    use crate::core::util::{test_dir, write_json};
    use crate::design::tokens::relative_path;

    fn bp(id: &str, width: u32, height: u32) -> Breakpoint {
        Breakpoint {
            id: id.into(),
            label: None,
            width,
            height,
        }
    }

    fn some(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    /// Shots dfw places in the project are written only inside it (a `.dfw/shots` link out of the
    /// project is refused when the PNG is written); a file the user names with --out is theirs.
    #[test]
    fn shots_in_the_project_carry_the_root_and_a_named_out_file_does_not() {
        let root = test_dir("shoot-inside");
        let one = [bp("desktop", 1440, 900)];
        let default = url_jobs(
            &root,
            "http://localhost:3000/",
            &one,
            &ShootOptions::default(),
        )
        .unwrap();
        assert_eq!(default[0].inside.as_deref(), Some(root.as_path()));
        let named = ShootOptions {
            out: Some("shot.png".into()),
            ..ShootOptions::default()
        };
        let named = url_jobs(&root, "http://localhost:3000/", &one, &named).unwrap();
        assert_eq!(named[0].inside, None);
    }

    #[test]
    fn url_jobs_name_their_files_like_cmd_shoot() {
        let root = test_dir("shoot-url");
        let both = [bp("mobile", 390, 844), bp("desktop", 1440, 900)];
        let outs = |url: &str, out: Option<&str>, bps: &[Breakpoint]| -> Vec<std::path::PathBuf> {
            let opts = ShootOptions {
                out: out.map(str::to_string),
                ..ShootOptions::default()
            };
            url_jobs(&root, url, bps, &opts)
                .unwrap()
                .into_iter()
                .map(|j| j.out)
                .collect()
        };
        let shot = |name: &str| root.join(rel::join(rel::SHOTS, file::SHOTS_URL)).join(name);
        assert_eq!(
            outs("http://localhost:3000/cart/items?x=1#top", None, &both),
            [
                shot("_cart_items__mobile.png"),
                shot("_cart_items__desktop.png")
            ]
        );
        assert_eq!(
            outs("http://localhost:3000", None, &both[..1]),
            [shot("___mobile.png")]
        );
        assert_eq!(outs("data:,", None, &both[..1]), [shot("___mobile.png")]);
        assert_eq!(
            outs("about:blank", None, &both[..1]),
            [shot("blank__mobile.png")]
        );
        assert_eq!(
            outs("http://x/", Some("shot.png"), &both),
            [resolve("shot__mobile.png"), resolve("shot__desktop.png")]
        );
        assert_eq!(
            outs("http://x/", Some("a.b/shot"), &both),
            [
                resolve("a.b/shot__mobile.png"),
                resolve("a.b/shot__desktop.png")
            ]
        );
        assert_eq!(
            outs("http://x/", Some("s-{bp}.png"), &both),
            [resolve("s-mobile.png"), resolve("s-desktop.png")]
        );
        assert_eq!(
            outs("http://x/", Some("one.png"), &both[..1]),
            [resolve("one.png")]
        );
        let opts = ShootOptions {
            viewport: true,
            selector: some("#a"),
            ..ShootOptions::default()
        };
        let job = &url_jobs(&root, "http://x/", &both[..1], &opts).unwrap()[0];
        assert_eq!(
            (
                job.full_page,
                job.selector.as_deref(),
                job.width,
                job.height
            ),
            (false, Some("#a"), 390, 844)
        );
        assert!(url_jobs(&root, "no scheme", &both, &ShootOptions::default()).is_err());
    }

    #[test]
    fn pathname_of_common_urls() {
        for (url, path) in [
            ("http://localhost:3000", "/"),
            ("http://localhost:3000/a/b.html?q#h", "/a/b.html"),
            ("HTTPS://example.test/x", "/x"),
            ("file:///tmp/a.html", "/tmp/a.html"),
            ("data:text/html,<p>hi</p>", "text/html,<p>hi</p>"),
            ("about:blank", "blank"),
        ] {
            assert_eq!(url_pathname(url).unwrap(), path, "{url}");
        }
        for bad in ["localhost:3000/x", "/just/a/path", "http://", ""] {
            // "localhost:3000/x" parses in Node too (scheme "localhost"); the others are invalid.
            if bad == "localhost:3000/x" {
                assert_eq!(url_pathname(bad).unwrap(), "3000/x");
            } else {
                assert!(url_pathname(bad).is_err(), "{bad}");
            }
        }
    }

    const BOTH: [(&str, u32, u32); 2] = [("mobile", 390, 844), ("desktop", 1440, 900)];

    fn both() -> Vec<Breakpoint> {
        BOTH.iter().map(|(id, w, h)| bp(id, *w, *h)).collect()
    }

    /// The Studio and its manifest are needed; a Studio whose process is gone does not count.
    #[test]
    fn studio_jobs_need_a_running_studio_and_a_manifest() {
        let root = test_dir("shoot-studio-errors");
        let p = project_paths(&root);
        let jobs = || studio_jobs(&root, &both(), &ShootOptions::default());
        assert_eq!(
            jobs().unwrap_err().message(),
            "Start the studio first (dfw studio start) or pass --url."
        );
        // This test process is alive: its pid stands in for the Studio's.
        write_json(
            &p.studio,
            &json!({ "pid": std::process::id(), "url": "http://127.0.0.1:4600/" }),
        )
        .unwrap();
        assert_eq!(
            jobs().unwrap_err().message(),
            format!("{} not found.", rel::MANIFEST)
        );
        write_json(&p.manifest, &json!({ "screens": [] })).unwrap();
        assert!(jobs().unwrap().is_empty());
        write_json(
            &p.studio,
            &json!({ "pid": 0, "url": "http://127.0.0.1:4600/" }),
        )
        .unwrap();
        assert!(jobs().is_err());
    }

    /// The shots folder of design version 3.
    const VERSION_3: &str = "v3";

    #[test]
    fn studio_jobs_cover_screens_variants_breakpoints_states_and_themes() {
        let v3 = |file: &str| rel::join(&rel::join(rel::SHOTS, VERSION_3), file);
        let root = test_dir("shoot-studio");
        let p = project_paths(&root);
        let both = both();
        let jobs = |opts: &ShootOptions| studio_jobs(&root, &both, opts);
        write_json(
            &p.studio,
            &json!({ "pid": std::process::id(), "url": "http://127.0.0.1:4600/" }),
        )
        .unwrap();
        write_json(
            &p.manifest,
            &json!({
                "states": ["empty", "error"],
                "themes": ["dark"],
                "pages": [{ "id": "shop", "screens": ["cart"] }],
                "screens": [
                    { "id": "cart", "variants": [{ "id": "a", "file": "screens/cart/a.html" }, { "id": "b", "file": "screens/cart/b.html" }] },
                    { "id": "admin/users", "page": "back", "states": ["loading state"], "variants": [{ "id": "a", "file": "screens/admin/users/a.html" }] },
                ],
            }),
        )
        .unwrap();
        write_json(&p.state, &json!({ "design": { "version": 3 } })).unwrap();
        let rel = |f: &Path| relative_path(&root, f);
        let list = |opts: ShootOptions| -> Vec<(String, String)> {
            jobs(&opts)
                .unwrap()
                .iter()
                .map(|j| (rel(&j.out), j.url.clone()))
                .collect()
        };
        let screen = |id: &str| ShootOptions {
            screen: some(id),
            ..ShootOptions::default()
        };
        let all = list(ShootOptions::default());
        assert_eq!(all.len(), 6);
        assert_eq!(
            all[0],
            (
                v3("cart__a__mobile.png"),
                format!(
                    "http://127.0.0.1:4600/{}",
                    rel::screen(&screen_page("cart"))
                )
            )
        );
        assert_eq!(all[5].0, v3("admin__users__a__desktop.png"));
        assert_eq!(
            list(ShootOptions {
                variant: some("b"),
                ..screen("cart")
            })
            .len(),
            2
        );
        let page = |id: &str| ShootOptions {
            page: some(id),
            ..ShootOptions::default()
        };
        assert_eq!(list(page("shop")).len(), 4);
        assert_eq!(list(page("back")).len(), 2);
        let states = list(ShootOptions {
            states: true,
            themes: true,
            ..screen("admin/users")
        });
        assert_eq!(
            states
                .iter()
                .map(|(o, u)| (o.clone(), u.rsplit('/').next().unwrap()))
                .collect::<Vec<_>>()[..2],
            [
                (v3("admin__users__a__mobile__dark.png"), "a.html?theme=dark"),
                (
                    v3("admin__users__a__mobile__dark__loading state.png"),
                    "a.html?theme=dark&state=loading+state"
                ),
            ]
        );
        assert_eq!(
            list(ShootOptions {
                states: true,
                ..screen("cart")
            })
            .len(),
            2 * 2 * 3
        );
        let one = list(ShootOptions {
            variant: some("a"),
            state: some("empty"),
            theme: some("dark"),
            selector: some("#main > .card[data-x]"),
            ..screen("cart")
        });
        assert_eq!(
            one[0].0,
            v3("cart__a__mobile__dark__empty__el-main_.card_data-x.png")
        );
        let system = ShootOptions {
            system: true,
            ..ShootOptions::default()
        };
        assert_eq!(
            list(system.clone()),
            [(
                v3("_system__mobile.png"),
                "http://127.0.0.1:4600/__dfw/system.html".to_string()
            )]
        );
        assert_eq!(
            list(ShootOptions {
                screen: some("cart"),
                ..system
            })
            .len(),
            1 + 4
        );
        assert!(jobs(&screen("nope")).unwrap().is_empty());
    }

    #[test]
    fn breakpoints_come_from_the_design_then_the_config() {
        let root = test_dir("shoot-bp");
        let ids = |bp: Option<&str>| -> Vec<String> {
            shoot_breakpoints(&root, bp)
                .unwrap()
                .into_iter()
                .map(|b| b.id)
                .collect()
        };
        assert_eq!(ids(None).len(), 3, "the config defaults");
        write_json(&project_paths(&root).manifest, &json!({ "breakpoints": [{ "id": "wide", "width": 1920, "height": 1080 }, { "id": "tiny", "width": 320, "height": 480 }] })).unwrap();
        assert_eq!(ids(None), ["wide", "tiny"]);
        assert_eq!(ids(Some("tiny,nope")), ["tiny"]);
        assert_eq!(ids(Some("nope")), Vec::<String>::new());
    }
}
