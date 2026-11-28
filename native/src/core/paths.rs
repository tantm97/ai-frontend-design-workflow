//! Path helpers: plugin root, global and project directories, project root search, real paths,
//! and the check every path built from an id or a request must pass.

use std::env;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};

use super::error::DfwError;

mod layout;

pub use self::layout::{file, in_design, project_env, rel};

/// Manifest files a dfw plugin may carry: Claude Code format, a root `plugin.json` (Copilot and
/// others), or `manifest.json` (`core/` in the development repository).
pub const MANIFESTS: [&str; 3] = [CLAUDE_MANIFEST, HOST_MANIFEST, DEV_MANIFEST];
/// The Claude Code manifest of a plugin folder.
pub const CLAUDE_MANIFEST: &str = ".claude-plugin/plugin.json";
/// The manifest at the root of a plugin folder (Copilot and others).
pub const HOST_MANIFEST: &str = "plugin.json";
/// The manifest of `core/` in the development repository.
pub const DEV_MANIFEST: &str = "manifest.json";
/// The key of a manifest that holds the plugin's version.
pub const MANIFEST_VERSION: &str = "version";

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
        let core = dir.join(DEV_CORE);
        if is_plugin_dir(&core) {
            return Some(core);
        }
    }
    None
}

/// The folder of a plugin that holds its launchers.
pub const PLUGIN_BIN: &str = "bin";
/// The launcher of a plugin folder, in [`PLUGIN_BIN`].
pub const LAUNCHER: &str = "dfw";
/// The plugin folder inside the development repository.
pub const DEV_CORE: &str = "core";

fn is_plugin_dir(dir: &Path) -> bool {
    dir.join(PLUGIN_BIN).join(LAUNCHER).is_file() && manifest_path(dir).is_some()
}

/// The project folder the environment names, if any: the first of [`project_env::PROJECT_DIR`]
/// that is set (dfw's own, then Claude Code's). Every command and every hook reads it here.
pub fn project_dir_from_env() -> Option<PathBuf> {
    project_env::PROJECT_DIR
        .iter()
        .find_map(|name| env_nonempty(name))
        .map(PathBuf::from)
}

/// Find the project root: the nearest ancestor containing `.dfw/state.json`, else the nearest
/// containing `.git`, else the start directory. `start` defaults to [`project_dir_from_env`],
/// then the working directory.
pub fn find_project_root(start: Option<&Path>) -> PathBuf {
    let start = match start {
        Some(p) => resolve(p),
        None => resolve(
            project_dir_from_env().unwrap_or_else(|| env::current_dir().unwrap_or_default()),
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

/// Whether paths are compared without case: macOS (APFS, HFS+) and Windows (NTFS) volumes ignore
/// case by default, so `design/Approved` is `design/approved` there. Decided per OS, without
/// testing the volume: on the rare case-sensitive volume of those systems it only refuses more.
pub const FOLD_CASE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// The real path of `p`: symbolic links and letter case as the file system has them. It also
/// works for a path that does not exist yet (its existing part is resolved, the rest is kept)
/// and for a link whose target is missing (the target is followed as written), so a dangling
/// link that points outside is seen as outside. `None` when the path cannot be resolved: a
/// cycle of links, or an entry that exists but cannot be read.
pub fn real_path(p: &Path) -> Option<PathBuf> {
    real_path_hops(&resolve(p), 40)
}

fn real_path_hops(p: &Path, hops: u32) -> Option<PathBuf> {
    if let Ok(real) = canonicalize(p) {
        return Some(real);
    }
    let (Some(up), Some(name)) = (p.parent(), p.file_name()) else {
        return Some(p.to_path_buf());
    };
    let up = real_path_hops(up, hops)?;
    let here = up.join(name);
    match std::fs::read_link(&here) {
        // A link whose target does not resolve: follow what it names.
        Ok(target) if hops > 0 => real_path_hops(&resolve(up.join(target)), hops - 1),
        // A cycle.
        Ok(_) => None,
        Err(_) if std::fs::symlink_metadata(&here).is_ok() => None,
        Err(_) => Some(here),
    }
}

/// Where a path is, compared with a root by real path ([`inside`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Under the root (or the root itself): the real path, and its part after the real root
    /// (empty for the root).
    Inside { real: PathBuf, rel: PathBuf },
    /// Elsewhere: the real path.
    Outside(PathBuf),
    /// The root or the path cannot be resolved (a cycle of links): the caller decides by the
    /// path as written, and never takes it for "outside".
    Unresolved,
}

/// The one "is it inside" check (`security-privacy.md` S2): `path` and `root` are both resolved
/// through every symbolic link (also one whose target is missing, [`real_path`]), so an alias
/// of the root (`/tmp` and `/private/tmp`) is the root and a link cannot lead out; components
/// are compared without case where the volume ignores case ([`FOLD_CASE`]). `..x.ts` is a name
/// like any other: only a real path elsewhere is outside.
pub fn inside(root: &Path, path: &Path) -> Place {
    let (Some(real_root), Some(real)) = (real_path(root), real_path(path)) else {
        return Place::Unresolved;
    };
    match strip_root(&real_root, &real, FOLD_CASE) {
        Some(rel) => Place::Inside { real, rel },
        None => Place::Outside(real),
    }
}

/// The part of `path` after `root`, component by component, without case when `fold` is set.
fn strip_root(root: &Path, path: &Path, fold: bool) -> Option<PathBuf> {
    let same = |a: Component, b: Component| {
        a == b
            || fold
                && a.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
    };
    let mut rest = path.components();
    for want in root.components() {
        if !rest.next().is_some_and(|got| same(want, got)) {
            return None;
        }
    }
    Some(rest.as_path().to_path_buf())
}

/// A file name part checked against its whole pattern: what every `*_file(name)` helper takes,
/// so an id from a request, a file or `.dfw/` state names a file only in the form dfw writes
/// (`security-privacy.md` S3). Made only by [`SafeName::matching`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeName(String);

impl SafeName {
    /// `text` when `pattern` matches all of it (not only a part), else `None`.
    pub fn matching(pattern: &regex::Regex, text: &str) -> Option<SafeName> {
        pattern
            .find(text)
            .filter(|m| m.start() == 0 && m.end() == text.len())
            .map(|_| SafeName(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SafeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Join a relative path that came from an id, a request or a file onto `root` and prove it stays
/// inside: no absolute path, no `..`, and its real path (through every symbolic link, also one
/// whose target is missing) is still under the real root, so a link cannot lead out ([`inside`]).
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
    // A root that does not exist holds nothing.
    canonicalize(root).map_err(|_| outside())?;
    let target = root.join(rel_path);
    match inside(root, &target) {
        Place::Outside(_) => Err(outside()),
        // A cycle of links ends; the path as written is inside.
        Place::Inside { .. } | Place::Unresolved => Ok(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{HTML_FILE, NESTED_PAGE, OTHER_CSS_FILE};
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
        let unix = p("/a/b");
        assert_eq!(strip_verbatim(unix.clone()), unix);
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
        let dfw = Path::new("r").join(file::DFW);
        let design = Path::new("r").join(file::DESIGN);
        assert_eq!(p.state, dfw.join("state.json"));
        assert_eq!(p.config_local, dfw.join("config.local.json"));
        assert_eq!(p.decisions, dfw.join("decisions.jsonl"));
        assert_eq!(p.manifest, design.join("design.json"));
        assert_eq!(p.approved, design.join("approved"));
        assert_eq!(p.handoff, design.join("handoff"));
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
        assert_eq!(rel::component(HTML_FILE), "design/system/components/a.html");
        assert!(rel::under(rel::COMPONENTS, &rel::component(HTML_FILE)));
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
        assert_eq!(rel::screen(NESTED_PAGE), "design/screens/a/b.html");
        assert_eq!(rel::design(OTHER_CSS_FILE), "design/x.css");
        assert_eq!(rel::approved("v1"), "design/approved/v1");
    }

    #[test]
    fn project_root_prefers_dfw_state_then_git_then_start() {
        let tmp = test_dir("root");
        let repo = tmp.join("repo");
        let app = repo.join("app");
        let deep = app.join("src");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            find_project_root(Some(&deep)),
            deep,
            "nothing found: the start directory"
        );
        fs::create_dir_all(repo.join(".git")).unwrap();
        assert_eq!(find_project_root(Some(&deep)), repo);
        fs::create_dir_all(app.join(file::DFW)).unwrap();
        assert_eq!(
            find_project_root(Some(&deep)),
            repo,
            ".dfw without state.json does not count"
        );
        fs::write(app.join(rel::STATE), "{}").unwrap();
        assert_eq!(find_project_root(Some(&deep)), app);
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn plugin_root_is_found_from_the_executable() {
        let tmp = test_dir("plugin");
        let exe = tmp
            .join("native")
            .join("target")
            .join("debug")
            .join(LAUNCHER);
        assert_eq!(plugin_root_from(&exe), None);
        let core = tmp.join(DEV_CORE);
        let bin = core.join(PLUGIN_BIN);
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(LAUNCHER), "").unwrap();
        assert_eq!(
            plugin_root_from(&exe),
            None,
            "a launcher without a manifest is not a plugin"
        );
        fs::write(core.join(DEV_MANIFEST), "{}").unwrap();
        assert_eq!(plugin_root_from(&exe), Some(core.clone()));
        assert_eq!(plugin_root_from(&bin.join("dfw-native")), Some(core));
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn resolve_inside_refuses_what_leaves_the_root() {
        let tmp = test_dir("inside");
        let root = tmp.join("project");
        let design = root.join(file::DESIGN);
        let elsewhere = tmp.join("elsewhere");
        fs::create_dir_all(&design).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        let new_page = rel::design(NESTED_PAGE);
        assert_eq!(
            resolve_inside(&root, &new_page).unwrap(),
            root.join(&new_page)
        );
        for bad in ["", "../elsewhere/x", "design/../../x", "/etc/passwd"] {
            assert!(resolve_inside(&root, bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            use crate::core::fixture::{HTML_FILE_B, LOOP_LINK};
            std::os::unix::fs::symlink(&elsewhere, design.join("link")).unwrap();
            assert!(resolve_inside(&root, "design/link/x.html").is_err());
            // A link whose target is missing is judged by where it points.
            std::os::unix::fs::symlink(elsewhere.join("gone.html"), design.join("dangling.html"))
                .unwrap();
            assert!(resolve_inside(&root, "design/dangling.html").is_err());
            std::os::unix::fs::symlink("../../elsewhere/no/dir", design.join("up")).unwrap();
            assert!(resolve_inside(&root, "design/up/x.html").is_err());
            let near = rel::design(HTML_FILE_B);
            std::os::unix::fs::symlink(HTML_FILE, root.join(&near)).unwrap();
            assert_eq!(
                resolve_inside(&root, &near).unwrap(),
                root.join(&near),
                "a dangling link that stays inside is fine"
            );
            std::os::unix::fs::symlink(LOOP_LINK, design.join(LOOP_LINK)).unwrap();
            assert!(
                resolve_inside(&root, &rel::design(LOOP_LINK)).is_ok(),
                "a cycle ends"
            );
        }
        fs::remove_dir_all(&tmp).unwrap();
    }

    /// The one "is it inside" check, over every case `security-privacy.md` S2 names.
    #[test]
    fn inside_decides_by_real_path() {
        /// A file whose name starts with two dots: a name, not a way up.
        const TWO_DOTS: &str = "..x.ts";
        /// The frozen folder with another case.
        const APPROVED_VARIANT: &str = "design/Approved/v1/a.html";
        let tmp = test_dir("inside-table");
        let root = tmp.join("project");
        let elsewhere = tmp.join("elsewhere");
        fs::create_dir_all(root.join(rel::approved("v1"))).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        let is_in = |r: &Path, p: &Path| matches!(inside(r, p), Place::Inside { .. });
        let is_out = |r: &Path, p: &Path| matches!(inside(r, p), Place::Outside(_));
        assert!(is_out(&root, &root.join("..")), "..");
        assert!(is_out(&root, &root.join("..").join("x")), "../x");
        assert!(is_out(&root, &elsewhere.join(HTML_FILE)), "absolute");
        assert_eq!(
            inside(&root, &root.join(TWO_DOTS)),
            Place::Inside {
                real: root.join(TWO_DOTS),
                rel: PathBuf::from(TWO_DOTS)
            }
        );
        assert_eq!(
            inside(&root, &root),
            Place::Inside {
                real: root.clone(),
                rel: PathBuf::new()
            },
            "the root itself"
        );
        let approved = root.join(rel::APPROVED);
        assert_eq!(
            is_in(&approved, &root.join(APPROVED_VARIANT)),
            FOLD_CASE,
            "a case variant of approved is approved where the volume ignores case"
        );
        // Also before the folder exists (the write would make it): the case is compared.
        let fresh = tmp.join("fresh");
        assert_eq!(
            is_in(&fresh.join(rel::APPROVED), &fresh.join(APPROVED_VARIANT)),
            FOLD_CASE,
            "a case variant of a folder not made yet"
        );
        let (upper, lower) = (Path::new("/a/B"), Path::new("/a/b/c"));
        assert_eq!(strip_root(upper, lower, true), Some(PathBuf::from("c")));
        assert_eq!(strip_root(upper, lower, false), None);
        #[cfg(unix)]
        {
            use crate::core::fixture::LOOP_LINK;
            use std::os::unix::fs::symlink;
            /// A link out of the project, and one whose target is missing.
            const OUT_LINK: &str = "out";
            const DANGLING_OUT: &str = "dangling-out.html";
            symlink(&elsewhere, root.join(OUT_LINK)).unwrap();
            assert!(
                is_out(&root, &root.join(OUT_LINK).join(HTML_FILE)),
                "a link out"
            );
            symlink(elsewhere.join(HTML_FILE), root.join(DANGLING_OUT)).unwrap();
            assert!(
                is_out(&root, &root.join(DANGLING_OUT)),
                "a dangling link out"
            );
            // An alias of the root (`/tmp` and `/private/tmp` on macOS) is the root.
            let alias = tmp.join("alias");
            symlink(&root, &alias).unwrap();
            assert!(is_in(&root, &alias.join(HTML_FILE)));
            assert!(is_in(&alias, &root.join(HTML_FILE)));
            symlink(LOOP_LINK, root.join(LOOP_LINK)).unwrap();
            assert_eq!(
                inside(&root, &root.join(LOOP_LINK).join(HTML_FILE)),
                Place::Unresolved,
                "a cycle of links"
            );
        }
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_safe_name_matches_its_whole_pattern() {
        const ID: &str = "px_1";
        let pattern = regex::Regex::new("px_[0-9]+").unwrap();
        assert_eq!(
            SafeName::matching(&pattern, ID).map(|n| n.to_string()),
            Some(ID.into())
        );
        for bad in ["px_", "../px_1", "px_1/..", "px_1\n", ""] {
            assert!(SafeName::matching(&pattern, bad).is_none(), "{bad:?}");
        }
    }
}
