//! Path helpers: plugin root, global and project directories, project root search, real paths,
//! and the check every path built from an id or a request must pass.

use std::env;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};

use super::error::DfwError;

mod layout;

pub use self::layout::{file, in_design, rel};

/// Manifest files a dfw plugin may carry: Claude Code format, a root `plugin.json` (Copilot and
/// others), or `manifest.json` (`core/` in the development repository).
pub const MANIFESTS: [&str; 3] = [".claude-plugin/plugin.json", "plugin.json", "manifest.json"];

/// An environment variable that is set and not empty (what JavaScript treats as truthy).
pub fn env_nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

/// `path.resolve(p)`: absolute and normalised without touching the file system.
pub fn resolve(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `toPosix`: the platform separator replaced by `/`.
pub fn to_posix(p: &str) -> String {
    p.split(MAIN_SEPARATOR).collect::<Vec<_>>().join("/")
}

/// Global dfw config directory (survives plugin uninstall): `DFW_HOME`, else
/// `$XDG_CONFIG_HOME/dfw`, else `~/.config/dfw`.
pub fn global_dir() -> PathBuf {
    if let Some(home) = env_nonempty("DFW_HOME") {
        return PathBuf::from(home);
    }
    let xdg = env_nonempty("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"));
    xdg.join("dfw")
}

/// Claude Code config directory (respects `CLAUDE_CONFIG_DIR`).
pub fn claude_dir() -> PathBuf {
    env_nonempty("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude"))
}

fn home_dir() -> PathBuf {
    env::home_dir().unwrap_or_default()
}

/// Path of a plugin directory's manifest, or `None`.
pub fn manifest_path(dir: &Path) -> Option<PathBuf> {
    MANIFESTS.iter().map(|m| dir.join(m)).find(|f| f.exists())
}

/// The plugin folder (the one holding `bin/`, `studio/`, `templates/`...): `DFW_PLUGIN_ROOT` when
/// set (the launcher passes it), else found from the executable: an ancestor that is a plugin
/// folder, or an ancestor holding `core/` (a build inside the development repository).
pub fn plugin_root() -> Option<PathBuf> {
    if let Some(root) = env_nonempty("DFW_PLUGIN_ROOT") {
        return Some(resolve(root));
    }
    let exe = env::current_exe().ok()?;
    let exe = canonicalize(&exe).unwrap_or(exe);
    plugin_root_from(&exe)
}

/// The search of [`plugin_root`] from a given executable path. Six levels cover
/// `<repo>/native/target/<triple>/<profile>/dfw` and stop a stray binary from adopting a far ancestor.
pub fn plugin_root_from(exe: &Path) -> Option<PathBuf> {
    for dir in exe.ancestors().skip(1).take(6) {
        if is_plugin_dir(dir) {
            return Some(dir.to_path_buf());
        }
        let core = dir.join("core");
        if is_plugin_dir(&core) {
            return Some(core);
        }
    }
    None
}

fn is_plugin_dir(dir: &Path) -> bool {
    dir.join("bin").join("dfw").is_file() && manifest_path(dir).is_some()
}

/// Find the project root: the nearest ancestor containing `.dfw/state.json`, else the nearest
/// containing `.git`, else the start directory. `start` defaults to `CLAUDE_PROJECT_DIR`, then
/// the working directory.
pub fn find_project_root(start: Option<&Path>) -> PathBuf {
    let start = match start {
        Some(p) => resolve(p),
        None => resolve(
            env_nonempty("CLAUDE_PROJECT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| env::current_dir().unwrap_or_default()),
        ),
    };
    let mut git_root: Option<PathBuf> = None;
    for dir in start.ancestors() {
        if dir.join(file::DFW).join(file::STATE).exists() {
            return dir.to_path_buf();
        }
        if git_root.is_none() && dir.join(".git").exists() {
            git_root = Some(dir.to_path_buf());
        }
    }
    git_root.unwrap_or(start)
}

/// Well-known files and folders of a project (`projectPaths`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub dfw: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
    pub config_local: PathBuf,
    pub decisions: PathBuf,
    pub reviews: PathBuf,
    pub stack: PathBuf,
    pub studio: PathBuf,
    pub checks: PathBuf,
    pub design: PathBuf,
    pub manifest: PathBuf,
    pub tokens: PathBuf,
    pub approved: PathBuf,
    pub handoff: PathBuf,
    /// `design/system`.
    pub system: PathBuf,
    /// `design/system/components`: the masters.
    pub components: PathBuf,
    /// `design/system/icons`.
    pub icons: PathBuf,
    /// `design/system/head.html`: the font links every page loads.
    pub head: PathBuf,
    /// `design/system/style.json`: the applied style guide.
    pub style: PathBuf,
    /// `design/system/inventory.json`.
    pub inventory: PathBuf,
    /// `design/system/components.css`, generated from the masters.
    pub components_css: PathBuf,
    /// `design/tokens/tokens.json`.
    pub tokens_json: PathBuf,
    /// `design/tokens/tokens.css`, generated from the tokens.
    pub tokens_css: PathBuf,
    /// `design/tokens/themes`.
    pub themes: PathBuf,
    /// `design/screens`.
    pub screens: PathBuf,
    /// `design/styles`.
    pub styles: PathBuf,
    /// `design/styles/base.css`: the shared page styles.
    pub base_css: PathBuf,
    /// `.dfw/build`: composed pages.
    pub build: PathBuf,
    /// `.dfw/shots`: screenshots.
    pub shots: PathBuf,
    /// `.dfw/edits`: the undo journal of direct edits.
    pub edits: PathBuf,
    /// `.dfw/penpot`: exports, imports and threads of the Penpot bridge.
    pub penpot: PathBuf,
    /// `.dfw/presence.json`: what the agent is editing now.
    pub presence: PathBuf,
    /// `.dfw/layout-cache.json`.
    pub layout_cache: PathBuf,
}

/// `projectPaths(root)`.
pub fn project_paths(root: &Path) -> ProjectPaths {
    let dfw = root.join(file::DFW);
    let design = root.join(file::DESIGN);
    let system = design.join(file::SYSTEM);
    let tokens = design.join(file::TOKENS);
    let styles = design.join(file::STYLES);
    ProjectPaths {
        root: root.to_path_buf(),
        state: dfw.join(file::STATE),
        config: dfw.join(file::CONFIG),
        config_local: dfw.join(file::CONFIG_LOCAL),
        decisions: dfw.join(file::DECISIONS),
        reviews: dfw.join(file::REVIEWS),
        stack: dfw.join(file::STACK),
        studio: dfw.join(file::STUDIO),
        checks: dfw.join(file::CHECKS),
        build: dfw.join(file::BUILD),
        shots: dfw.join(file::SHOTS),
        edits: dfw.join(file::EDITS),
        penpot: dfw.join(file::PENPOT),
        presence: dfw.join(file::PRESENCE),
        layout_cache: dfw.join(file::LAYOUT_CACHE),
        manifest: design.join(file::MANIFEST),
        approved: design.join(file::APPROVED),
        handoff: design.join(file::HANDOFF),
        screens: design.join(file::SCREENS),
        base_css: styles.join(file::BASE_CSS),
        styles,
        components: system.join(file::COMPONENTS),
        icons: system.join(file::ICONS),
        head: system.join(file::HEAD),
        style: system.join(file::STYLE),
        inventory: system.join(file::INVENTORY),
        components_css: system.join(file::COMPONENTS_CSS),
        tokens_json: tokens.join(file::TOKENS_JSON),
        tokens_css: tokens.join(file::TOKENS_CSS),
        themes: tokens.join(file::THEMES),
        system,
        tokens,
        dfw,
        design,
    }
}

/// `fs::canonicalize` without the Windows verbatim prefix: `\\?\C:\x` is `C:\x` and
/// `\\?\UNC\host\share` is `\\host\share`. The prefix would reach messages, JSON and
/// child processes, and a path with it never `starts_with` the same path without it. The
/// standard library adds it again by itself where a long path needs it.
pub fn canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    p.canonicalize().map(strip_verbatim)
}

/// The path without the Windows verbatim prefix of a drive or UNC path (unchanged elsewhere).
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return p;
    }
    let Some(text) = p.to_str() else {
        return p;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

/// `pathToFileURL(path).href`: the absolute path percent-encoded as Node does it. A Windows
/// drive path is `file:///C:/x` and a UNC path `file://host/share/x`.
pub fn file_url(path: &Path) -> String {
    let resolved = strip_verbatim(resolve(path)).to_string_lossy().into_owned();
    let text = if cfg!(windows) {
        resolved.replace('\\', "/")
    } else {
        resolved
    };
    let mut url = String::from("file://");
    let text = match text.strip_prefix("//") {
        Some(unc) => {
            let end = unc.find('/').unwrap_or(unc.len());
            url.push_str(&unc[..end]);
            &unc[end..]
        }
        None => text.as_str(),
    };
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~!$&'()*+,;=:@".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// `fileURLToPath` for the `file:` URLs Chrome reports.
pub fn file_url_to_path(url: &str) -> PathBuf {
    let rest = url.strip_prefix("file:").unwrap_or(url);
    // An authority (`//host`) comes before the path; a local file has an empty one.
    let rest = match rest.strip_prefix("//") {
        Some(after) => &after[after.find('/').unwrap_or(after.len())..],
        None => rest,
    };
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| rest.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    // "/C:/x" is "C:\x" on Windows.
    if cfg!(windows) {
        PathBuf::from(path.trim_start_matches('/').replace('/', "\\"))
    } else {
        PathBuf::from(path)
    }
}

/// The real path of `p`: symbolic links and letter case as the file system has them. It also
/// works for a path that does not exist yet (its existing part is resolved, the rest is kept)
/// and for a link whose target is missing (the target is followed as written), so a dangling
/// link that points outside is seen as outside.
pub fn real_path(p: &Path) -> PathBuf {
    real_path_hops(&resolve(p), 40)
}

fn real_path_hops(p: &Path, hops: u32) -> PathBuf {
    if let Ok(real) = canonicalize(p) {
        return real;
    }
    let (Some(up), Some(name)) = (p.parent(), p.file_name()) else {
        return p.to_path_buf();
    };
    let up = real_path_hops(up, hops);
    let here = up.join(name);
    match std::fs::read_link(&here) {
        // A link whose target does not resolve: follow what it names (a cycle stops after `hops`).
        Ok(target) if hops > 0 => real_path_hops(&resolve(up.join(target)), hops - 1),
        _ => here,
    }
}

/// Join a relative path that came from an id, a request or a file onto `root` and prove it stays
/// inside: no absolute path, no `..`, and its real path (through every symbolic link, also one
/// whose target is missing) is still under the real root, so a link cannot lead out.
pub fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf, DfwError> {
    let outside = || DfwError::new(format!("Path \"{rel}\" is outside the project."));
    let rel_path = Path::new(rel);
    if rel.is_empty()
        || rel.contains('\0')
        || rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(outside());
    }
    let target = root.join(rel_path);
    let real_root = canonicalize(root).map_err(|_| outside())?;
    if !real_path(&target).starts_with(&real_root) {
        return Err(outside());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::fs;

    /// File URLs both ways, as the browser and Node write them, for this platform's paths.
    #[test]
    fn file_urls_are_percent_encoded_both_ways() {
        let (abs, dotted, base) = if cfg!(windows) {
            (
                r"C:\tmp\a b\é#1?[x]|.html",
                r"C:\x\..\y\100%.html",
                "file:///C:",
            )
        } else {
            ("/tmp/a b/é#1?[x]|.html", "/x/../y/100%.html", "file://")
        };
        let url = file_url(Path::new(abs));
        assert_eq!(
            url,
            format!("{base}/tmp/a%20b/%C3%A9%231%3F%5Bx%5D%7C.html")
        );
        assert_eq!(file_url(Path::new(dotted)), format!("{base}/y/100%25.html"));
        assert_eq!(file_url_to_path(&url), Path::new(abs));
        if cfg!(windows) {
            assert_eq!(
                file_url(Path::new(r"\\?\C:\a b.html")),
                "file:///C:/a%20b.html",
                "no verbatim prefix in a URL"
            );
            assert_eq!(
                file_url(Path::new(r"\\host\share\a.html")),
                "file://host/share/a.html"
            );
        }
    }

    #[test]
    fn strip_verbatim_keeps_drive_and_unc_paths_usable() {
        let p = |s: &str| PathBuf::from(s);
        if cfg!(windows) {
            assert_eq!(strip_verbatim(p(r"\\?\C:\a\b")), p(r"C:\a\b"));
            assert_eq!(strip_verbatim(p(r"\\?\UNC\h\s\x")), p(r"\\h\s\x"));
            assert_eq!(strip_verbatim(p(r"\\?\Volume{1}\x")), p(r"\\?\Volume{1}\x"));
        }
        assert_eq!(strip_verbatim(p("/a/b")), p("/a/b"));
        let dir = test_dir("canonical");
        assert!(!canonicalize(&dir)
            .unwrap()
            .to_string_lossy()
            .starts_with(r"\\?\"));
    }

    #[test]
    fn resolve_normalises_without_the_file_system() {
        let cwd = env::current_dir().unwrap();
        assert_eq!(resolve("a/./b/../c"), cwd.join("a").join("c"));
        assert_eq!(resolve(cwd.join("x").join("..")), cwd);
    }

    #[test]
    fn to_posix_replaces_the_platform_separator() {
        let native = ["design", "screens", "a.html"].join(std::path::MAIN_SEPARATOR_STR);
        assert_eq!(to_posix(&native), "design/screens/a.html");
    }

    #[test]
    fn project_paths_has_the_node_fields() {
        let p = project_paths(Path::new("r"));
        assert_eq!(p.state, Path::new("r").join(".dfw").join("state.json"));
        assert_eq!(
            p.config_local,
            Path::new("r").join(".dfw").join("config.local.json")
        );
        assert_eq!(
            p.decisions,
            Path::new("r").join(".dfw").join("decisions.jsonl")
        );
        assert_eq!(
            p.manifest,
            Path::new("r").join("design").join("design.json")
        );
        assert_eq!(p.approved, Path::new("r").join("design").join("approved"));
        assert_eq!(p.handoff, Path::new("r").join("design").join("handoff"));
    }

    /// The paths of the design system and their project-relative spellings name one place.
    #[test]
    fn design_system_paths_match_their_relative_names() {
        let root = Path::new("r");
        let p = project_paths(root);
        let pairs = [
            (&p.system, rel::SYSTEM),
            (&p.tokens, rel::TOKENS),
            (&p.components, rel::COMPONENTS),
            (&p.icons, rel::ICONS),
            (&p.themes, rel::THEMES),
            (&p.head, rel::HEAD),
            (&p.style, rel::STYLE),
            (&p.inventory, rel::INVENTORY),
            (&p.components_css, rel::COMPONENTS_CSS),
            (&p.tokens_json, rel::TOKENS_JSON),
        ];
        for (path, name) in pairs {
            assert_eq!(*path, root.join(name), "{name}");
        }
        assert_eq!(p.tokens_css, p.design.join(in_design::TOKENS_CSS));
        for (inside, name) in [
            (in_design::HEAD, rel::HEAD),
            (in_design::COMPONENTS_CSS, rel::COMPONENTS_CSS),
            (in_design::TOKENS_JSON, rel::TOKENS_JSON),
        ] {
            assert_eq!(format!("{}/{inside}", file::DESIGN), name);
        }
        for name in [
            rel::BRIEF,
            rel::STYLE_SOURCE,
            rel::EXTRACTED_TOKENS,
            rel::SYSTEM_PAGE,
        ] {
            assert!(rel::under(rel::SYSTEM, name), "{name}");
        }
        assert_eq!(rel::component("a.html"), "design/system/components/a.html");
        assert!(rel::under(rel::COMPONENTS, &rel::component("a.html")));
        assert!(!rel::under(rel::SYSTEM, "design/systemx/a"));
        assert!(!rel::under(rel::SYSTEM, rel::SYSTEM));
    }

    #[test]
    fn project_paths_match_their_relative_names() {
        let root = Path::new("r");
        let p = project_paths(root);
        let pairs = [
            (&p.manifest, rel::MANIFEST),
            (&p.screens, rel::SCREENS),
            (&p.styles, rel::STYLES),
            (&p.base_css, rel::BASE_CSS),
            (&p.approved, rel::APPROVED),
            (&p.handoff, rel::HANDOFF),
            (&p.dfw, rel::DFW),
            (&p.state, rel::STATE),
            (&p.config, rel::CONFIG),
            (&p.config_local, rel::CONFIG_LOCAL),
            (&p.decisions, rel::DECISIONS),
            (&p.stack, rel::STACK),
            (&p.reviews, rel::REVIEWS),
            (&p.build, rel::BUILD),
            (&p.shots, rel::SHOTS),
            (&p.edits, rel::EDITS),
            (&p.penpot, rel::PENPOT),
        ];
        for (path, name) in pairs {
            assert_eq!(*path, root.join(name), "{name}");
        }
        assert_eq!(p.design, root.join(rel::DESIGN));
        assert_eq!(p.presence, root.join(".dfw/presence.json"));
        assert_eq!(p.layout_cache, root.join(".dfw/layout-cache.json"));
        assert_eq!(p.studio, root.join(".dfw/studio.json"));
        assert_eq!(p.checks, root.join(".dfw/checks"));
        assert_eq!(
            p.penpot.join(file::PENPOT_EXPORT),
            root.join(rel::PENPOT_EXPORT)
        );
        assert_eq!(
            p.penpot.join(file::PENPOT_TOKENS),
            root.join(rel::PENPOT_TOKENS)
        );
        assert_eq!(
            format!("{}/{}", file::DESIGN, in_design::BASE_CSS),
            rel::BASE_CSS
        );
        assert_eq!(rel::screen("a/b.html"), "design/screens/a/b.html");
        assert_eq!(rel::design("x.css"), "design/x.css");
        assert_eq!(rel::approved("v1"), "design/approved/v1");
    }

    #[test]
    fn project_root_prefers_dfw_state_then_git_then_start() {
        let tmp = test_dir("root");
        let deep = tmp.join("repo").join("app").join("src");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            find_project_root(Some(&deep)),
            deep,
            "nothing found: the start directory"
        );
        fs::create_dir_all(tmp.join("repo").join(".git")).unwrap();
        assert_eq!(find_project_root(Some(&deep)), tmp.join("repo"));
        fs::create_dir_all(tmp.join("repo").join("app").join(".dfw")).unwrap();
        assert_eq!(
            find_project_root(Some(&deep)),
            tmp.join("repo"),
            ".dfw without state.json does not count"
        );
        fs::write(
            tmp.join("repo").join("app").join(".dfw").join("state.json"),
            "{}",
        )
        .unwrap();
        assert_eq!(find_project_root(Some(&deep)), tmp.join("repo").join("app"));
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn plugin_root_is_found_from_the_executable() {
        let tmp = test_dir("plugin");
        let exe = tmp.join("native").join("target").join("debug").join("dfw");
        assert_eq!(plugin_root_from(&exe), None);
        fs::create_dir_all(tmp.join("core").join("bin")).unwrap();
        fs::write(tmp.join("core").join("bin").join("dfw"), "").unwrap();
        assert_eq!(
            plugin_root_from(&exe),
            None,
            "a launcher without a manifest is not a plugin"
        );
        fs::write(tmp.join("core").join("manifest.json"), "{}").unwrap();
        assert_eq!(plugin_root_from(&exe), Some(tmp.join("core")));
        assert_eq!(
            plugin_root_from(&tmp.join("core").join("bin").join("dfw-native")),
            Some(tmp.join("core"))
        );
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn resolve_inside_refuses_what_leaves_the_root() {
        let tmp = test_dir("inside");
        fs::create_dir_all(tmp.join("project").join("design")).unwrap();
        fs::create_dir_all(tmp.join("elsewhere")).unwrap();
        let root = tmp.join("project");
        assert_eq!(
            resolve_inside(&root, "design/new/a.html").unwrap(),
            root.join("design/new/a.html")
        );
        for bad in ["", "../elsewhere/x", "design/../../x", "/etc/passwd"] {
            assert!(resolve_inside(&root, bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(tmp.join("elsewhere"), root.join("design").join("link"))
                .unwrap();
            assert!(resolve_inside(&root, "design/link/x.html").is_err());
            // A link whose target is missing is judged by where it points.
            std::os::unix::fs::symlink(
                tmp.join("elsewhere").join("gone.html"),
                root.join("design").join("dangling.html"),
            )
            .unwrap();
            assert!(resolve_inside(&root, "design/dangling.html").is_err());
            std::os::unix::fs::symlink("../../elsewhere/no/dir", root.join("design").join("up"))
                .unwrap();
            assert!(resolve_inside(&root, "design/up/x.html").is_err());
            std::os::unix::fs::symlink("a.html", root.join("design").join("near.html")).unwrap();
            assert_eq!(
                resolve_inside(&root, "design/near.html").unwrap(),
                root.join("design/near.html"),
                "a dangling link that stays inside is fine"
            );
            std::os::unix::fs::symlink("loop", root.join("design").join("loop")).unwrap();
            assert!(resolve_inside(&root, "design/loop").is_ok(), "a cycle ends");
        }
        fs::remove_dir_all(&tmp).unwrap();
    }
}
