//! The shape of `design/design.json`, in one place: the name of every key, and borrowed views
//! (`Manifest`, `Screen`, `Variant`) whose accessors read it. Readers use
//! these instead of `value.get("screens")`, so a key is spelled once.
//!
//! Two ways to read a value of the wrong type, both kept from the Node CLI on purpose (the golden
//! suite `checks` holds them):
//! - `…_checked` accessors report it (`dfw lint`, the gates): a list that is not a list, or a
//!   `null` item, is a [`DfwError`] that names `design/design.json`;
//! - the plain accessors skip it (the Studio, Penpot, caches): a half-written file never breaks
//!   a canvas.

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::rel;

/// The property Node's `x?.length` reads: an object with a truthy `length` passed that test.
const JS_LENGTH: &str = "length";

/// `design/design.json` keys.
pub mod key {
    /// The product name.
    pub const PRODUCT: &str = "product";
    pub const BREAKPOINTS: &str = "breakpoints";
    pub const STATES: &str = "states";
    pub const PAGES: &str = "pages";
    pub const FLOWS: &str = "flows";
    pub const SCREENS: &str = "screens";
    pub const VARIANTS: &str = "variants";
    pub const SECTIONS: &str = "sections";
    pub const NOTES: &str = "notes";
    pub const ID: &str = "id";
    /// The title of a page.
    pub const TITLE: &str = "title";
    pub const DESCRIPTION: &str = "description";
    /// The label of a variant.
    pub const LABEL: &str = "label";
    pub const FILE: &str = "file";
    pub const BLUEPRINT: &str = "blueprint";
    /// What the product is: `product` (app screens, the default) or `marketing`.
    pub const SURFACE: &str = "surface";
    /// The token themes the Studio offers (`dark`, the option themes `opt-<name>`).
    pub const THEMES: &str = "themes";
    /// A screen left out of the canvas.
    pub const ARCHIVED: &str = "archived";
    /// The canvas page a screen names for itself.
    pub const PAGE: &str = "page";
    /// The screen a flow (or a link between screens) starts from.
    pub const FROM: &str = "from";
    /// The screen a flow (or a link between screens) goes to.
    pub const TO: &str = "to";
    /// The id of the variant a screen's approval chose.
    pub const APPROVED_VARIANT: &str = "approvedVariant";
    /// A variant the agent recommends.
    pub const RECOMMENDED: &str = "recommended";
    /// The app route a screen becomes.
    pub const ROUTE: &str = "route";
    /// Added by the Studio's manifest API: the states a variant designs.
    pub const DESIGNED_STATES: &str = "designedStates";
    /// Added by the Studio's manifest API: the canvas pages.
    pub const STUDIO_PAGES: &str = "studioPages";
    /// Added by the Studio's manifest API: `true` when there is no `design.json` and the
    /// screens were read from `design/screens/**`.
    pub const INFERRED: &str = "inferred";
}

/// The keys of a screen's [`key::BLUEPRINT`]: the plan of the screen before it is drawn.
pub mod blueprint {
    /// The layout pattern.
    pub const LAYOUT: &str = "layout";
    /// What the user comes to the screen to do.
    pub const GOAL: &str = "goal";
    pub const PRIMARY_ACTION: &str = "primaryAction";
    /// What must show without scrolling.
    pub const ABOVE_FOLD: &str = "aboveFold";
    /// The regions of the screen: `{ priority, name, content }`.
    pub const REGIONS: &str = "regions";
    pub const PRIORITY: &str = "priority";
    pub const NAME: &str = "name";
    pub const CONTENT: &str = "content";
    /// How the screen looks with no, typical and the most data: `{ empty, typical, max }`.
    pub const DATA: &str = "data";
    /// How the screen answers the user's actions.
    pub const FEEDBACK: &str = "feedback";
}

/// A `design.json` value of the wrong type, as `dfw lint` reports it.
pub fn malformed(what: &str) -> DfwError {
    super::malformed(rel::MANIFEST, what)
}

/// `value || []` before an array method, the skipping way: the items of a list, nothing for
/// anything else.
pub fn items(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

/// `value || []` before an array method, the reporting way: a list, nothing for a missing or
/// falsy value, and an error (Node's TypeError) for any other value.
pub fn items_checked<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a [Value], DfwError> {
    match value {
        Some(Value::Array(items)) => Ok(items),
        Some(other) if js::truthy(Some(other)) => Err(malformed(&format!("{what} must be a list"))),
        _ => Ok(&[]),
    }
}

/// An item whose property the Node code read: `null` was a TypeError there.
pub fn not_null<'a>(value: &'a Value, what: &str) -> Result<&'a Value, DfwError> {
    if value.is_null() {
        return Err(malformed(&format!("{what} must not be null")));
    }
    Ok(value)
}

/// The whole file.
#[derive(Debug, Clone, Copy)]
pub struct Manifest<'a>(pub &'a Value);

impl<'a> Manifest<'a> {
    /// The screens (skipping a value of the wrong type).
    pub fn screens(self) -> impl Iterator<Item = Screen<'a>> {
        items(self.0.get(key::SCREENS)).iter().map(Screen)
    }
    /// `(m?.screens || []).find((s) => (s.variants || []).some((v) => v.file === file))`: the
    /// screen that owns a file. Like `find` and `some`, it stops at the first match, so only what
    /// comes before it has to be well-formed (the reporting way).
    pub fn screen_of_file_checked(self, file: &str) -> Result<Option<Screen<'a>>, DfwError> {
        for screen in items_checked(self.0.get(key::SCREENS), "\"screens\"")? {
            let screen = Screen(not_null(screen, "a screen")?);
            for variant in items_checked(screen.0.get(key::VARIANTS), "\"variants\" of a screen")? {
                if not_null(variant, "a variant")?
                    .get(key::FILE)
                    .and_then(Value::as_str)
                    == Some(file)
                {
                    return Ok(Some(screen));
                }
            }
        }
        Ok(None)
    }
}

/// One screen.
#[derive(Debug, Clone, Copy)]
pub struct Screen<'a>(pub &'a Value);

impl<'a> Screen<'a> {
    pub fn value(self) -> &'a Value {
        self.0
    }
    /// `String(s.id)`, for messages and the private check.
    pub fn id_text(self) -> String {
        js::to_string(self.0.get(key::ID))
    }
    /// `String(sc.id).startsWith('_')`: screens the workflow keeps out of the checks
    /// (direction explorations).
    pub fn is_private(self) -> bool {
        self.id_text().starts_with('_')
    }
    pub fn variants(self) -> impl Iterator<Item = Variant<'a>> {
        items(self.0.get(key::VARIANTS)).iter().map(Variant)
    }
    /// `sc.states?.length ? sc.states : m.states || []` (the reporting way): its own non-empty
    /// list, else the manifest's.
    pub fn declared_states_checked(self, m: Manifest<'a>) -> Result<&'a [Value], DfwError> {
        match self.0.get(key::STATES) {
            Some(Value::Array(own)) if !own.is_empty() => return Ok(own),
            Some(Value::String(text)) if !text.is_empty() => {
                return Err(malformed("\"states\" of a screen must be a list"));
            }
            Some(Value::Object(map)) if js::truthy(map.get(JS_LENGTH)) => {
                return Err(malformed("\"states\" of a screen must be a list"));
            }
            _ => {}
        }
        items_checked(m.0.get(key::STATES), "\"states\"")
    }
}

/// One variant of a screen.
#[derive(Debug, Clone, Copy)]
pub struct Variant<'a>(pub &'a Value);

impl<'a> Variant<'a> {
    pub fn value(self) -> &'a Value {
        self.0
    }
    /// The design file, relative to `design/` (user input: resolve it inside before reading).
    /// An empty name is returned as is, as Node read it.
    pub fn file(self) -> Option<&'a str> {
        self.0.get(key::FILE).and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{screen_page, HTML_FILE, SCREEN};
    use crate::core::paths::file;
    use serde_json::json;

    /// A state the manifest declares for every screen, and the cart's only one.
    const EMPTY: &str = "empty";
    /// A second state of every screen.
    const ERROR: &str = "error";
    /// A screen with its own states.
    const CART: &str = "cart";
    /// A screen kept out of the checks: a direction exploration.
    const EXPLORATION: &str = "_dir";

    /// The first page of the screen `id`, as the manifest names it (relative to `design/`).
    fn page_file(id: &str) -> String {
        rel::join(file::SCREENS, &screen_page(id))
    }

    #[test]
    fn views_read_a_typical_manifest() {
        let (home_file, cart_file) = (page_file(SCREEN), page_file(CART));
        let v = json!({
            "states": [EMPTY, ERROR],
            "screens": [
                { "id": SCREEN, "variants": [{ "file": home_file }, { "file": "" }] },
                { "id": CART, "states": [EMPTY], "variants": [{ "file": cart_file }] },
                { "id": EXPLORATION, "variants": [] }
            ]
        });
        let m = Manifest(&v);
        let ids: Vec<_> = m.screens().map(Screen::id_text).collect();
        assert_eq!(ids, [SCREEN, CART, EXPLORATION]);
        let home = m.screens().next().unwrap();
        let files: Vec<_> = home.variants().filter_map(Variant::file).collect();
        assert_eq!(files, [home_file.as_str(), ""]);
        assert_eq!(
            home.declared_states_checked(m).unwrap(),
            &[json!(EMPTY), json!(ERROR)],
            "inherited"
        );
        let cart = m.screen_of_file_checked(&cart_file).unwrap().unwrap();
        assert_eq!(
            cart.declared_states_checked(m).unwrap(),
            &[json!(EMPTY)],
            "its own"
        );
        assert!(m.screen_of_file_checked("nope.html").unwrap().is_none());
        assert!(m.screens().nth(2).unwrap().is_private());
    }

    #[test]
    fn the_two_ways_to_read_a_wrong_shape() {
        let v = json!({ "screens": "home" });
        let m = Manifest(&v);
        assert_eq!(m.screens().count(), 0, "skipped");
        let err = m.screen_of_file_checked(HTML_FILE).unwrap_err();
        assert!(err.message().contains("\"screens\" must be a list"));
        let v = json!({ "screens": [null] });
        let err = Manifest(&v).screen_of_file_checked(HTML_FILE).unwrap_err();
        assert!(err.message().contains("a screen must not be null"));
        let v = json!({ "screens": [{ "id": "x", "states": "empty", "variants": [null] }] });
        let m = Manifest(&v);
        let s = m.screens().next().unwrap();
        assert!(s.declared_states_checked(m).is_err());
        assert!(m.screen_of_file_checked(HTML_FILE).is_err());
        // A falsy value is an empty list both ways (Node's `value || []`).
        let v = json!({ "screens": false });
        assert!(Manifest(&v)
            .screen_of_file_checked(HTML_FILE)
            .unwrap()
            .is_none());
    }
}
