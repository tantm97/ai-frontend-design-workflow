//! Merging the tokens of a Penpot set into a dfw tree (`mergeSet`): added, changed, removed
//! and kept tokens, with the notes of every lossy step.

use super::*;

fn inherited_type_at<'a>(tree: &'a Value, parts: &[&str]) -> Option<&'a Value> {
    let mut ty = None;
    let mut node = Some(tree);
    for part in parts {
        let Some(n) = node.filter(|n| n.is_object() || n.is_array()) else {
            return ty;
        };
        if let Some(t) = js::truthy_value(n.get("$type")) {
            ty = Some(t);
        }
        node = n.get(*part);
    }
    ty
}

fn is_token(node: &Value) -> bool {
    node.as_object().is_some_and(|m| m.contains_key("$value"))
}

/// Write one token into a dfw tree: the `$type` only when it differs from the group's, the
/// other keys of an existing token kept (`$extensions`, ...). A description is set when given
/// and removed when not.
fn set_token(
    tree: &mut Value,
    name: &str,
    ty: Option<&Value>,
    value: &Value,
    description: Option<&Value>,
) {
    let Some(parts) = token_path(name) else {
        return;
    };
    let Some((leaf, groups)) = parts.split_last() else {
        return;
    };
    if !tree.is_object() {
        *tree = json!({});
    }
    // First make the groups, then read the inherited type, then write the leaf.
    let mut node = &mut *tree;
    for part in groups {
        let Value::Object(map) = node else {
            return;
        };
        let slot = map.entry((*part).to_string()).or_insert(Value::Null);
        if !slot.is_object() || is_token(slot) {
            *slot = json!({});
        }
        node = slot;
    }
    let inherited = inherited_type_at(tree, &parts).cloned();
    let mut node = &mut *tree;
    for part in groups {
        let Some(next) = node.get_mut(*part) else {
            return;
        };
        node = next;
    }
    let Value::Object(group) = node else {
        return;
    };
    let prev = group.get(*leaf).filter(|v| is_token(v)).cloned();
    let mut next = prev
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match ty {
        Some(ty) if !js::strict_eq(Some(ty), inherited.as_ref()) => {
            next.insert("$type".into(), ty.clone());
        }
        _ => {
            next.shift_remove("$type");
        }
    }
    next.insert("$value".into(), value.clone());
    match js::truthy_value(description) {
        Some(d) => {
            next.insert("$description".into(), d.clone());
        }
        None => {
            next.shift_remove("$description");
        }
    }
    // Key order: $type, $value, $description, then anything else ($extensions...).
    let mut ordered = Map::new();
    for key in ["$type", "$value", "$description"] {
        if let Some(v) = next.get(key) {
            ordered.insert(key.to_string(), v.clone());
        }
    }
    for (key, v) in js::entries(&next) {
        if !ordered.contains_key(key) {
            ordered.insert(key.clone(), v.clone());
        }
    }
    group.insert((*leaf).to_string(), Value::Object(ordered));
}

/// Remove a token and the groups it leaves empty (the root stays).
fn delete_token(tree: &mut Value, name: &str) {
    /// True when the leaf was removed and every group below `node` on the way went with it.
    fn delete_in(node: &mut Map<String, Value>, parts: &[&str]) -> bool {
        let Some((first, rest)) = parts.split_first() else {
            return false;
        };
        if rest.is_empty() {
            node.shift_remove(*first);
            return true;
        }
        let Some(Value::Object(child)) = node.get_mut(*first) else {
            return false;
        };
        if !delete_in(child, rest) || child.keys().any(|k| !k.starts_with('$')) {
            return false;
        }
        node.shift_remove(*first);
        true
    }
    let parts: Vec<&str> = name.split('.').collect();
    if let Value::Object(root) = tree {
        delete_in(root, &parts);
    }
}

/// What an import changes in one set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetDiff {
    /// `{ token, type, value }`
    pub added: Vec<Value>,
    /// `{ token, type, from, to, descriptionOnly? }`
    pub changed: Vec<Value>,
    /// `{ token, type, value }`
    pub removed: Vec<Value>,
    /// `{ token, type }`: in dfw, missing in Penpot, kept
    pub kept: Vec<Value>,
}

impl SetDiff {
    pub(super) fn to_json(&self) -> Value {
        json!({ "added": self.added, "changed": self.changed, "removed": self.removed, "kept": self.kept })
    }

    pub(super) fn writes(&self) -> bool {
        !(self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty())
    }
}

/// The result of [`merge_set`].
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub tree: Value,
    pub diff: SetDiff,
    /// `{ set, token, type, reason }`
    pub unsupported: Vec<Value>,
    /// `{ set, token, note }`
    pub warnings: Vec<Value>,
}

/// `(a || undefined) === (b || undefined)` for two descriptions.
fn same_description(a: Option<&Value>, b: Option<&Value>) -> bool {
    js::strict_eq(js::truthy_value(a), js::truthy_value(b))
}

/// `mergeSet`: merge one Penpot set into a dfw tree. A token whose Penpot value equals what dfw
/// would export keeps dfw's exact value (lossless round trip).
pub fn merge_set(
    current: Option<&Value>,
    penpot_set: &Value,
    set: &str,
    prune: bool,
) -> Result<Merged, DfwError> {
    let empty = json!({});
    let current = js::truthy_value(current).unwrap_or(&empty);
    let mut tree = current.clone();
    let cur = flatten_tokens(current);
    let mut reserved = Vec::new();
    let incoming = walk_set(penpot_set, &mut reserved);
    let mut diff = SetDiff::default();
    let mut unsupported: Vec<Value> = reserved
        .into_iter()
        .map(|token| {
            json!({ "set": set, "token": token, "reason": "reserved name (__proto__, constructor, prototype)" })
        })
        .collect();
    let mut warnings = Vec::new();
    for (name, pt) in incoming.iter() {
        let c = cur.get(name);
        if let Some(c) = c {
            let exported = token_to_penpot_quiet(name, c);
            if exported.value().is_some_and(|v| same(v, &pt.value)) {
                if same_description(exported.description(), pt.description.as_ref()) {
                    continue; // unchanged
                }
                // Only the description was edited in Penpot: keep dfw's exact value (oklch, seconds, ...).
                set_token(
                    &mut tree,
                    name,
                    c.ty.as_ref(),
                    &c.value,
                    pt.description.as_ref(),
                );
                diff.changed.push(js::object(vec![
                    ("token", Some(name.into())),
                    ("type", c.ty.clone()),
                    ("from", Some(c.value.clone())),
                    ("to", Some(c.value.clone())),
                    ("descriptionOnly", Some(Value::Bool(true))),
                ]));
                continue;
            }
        }
        let mut notes = Vec::new();
        let converted = token_from_penpot(name, pt, c.and_then(|c| c.ty.as_ref()), &mut |w| {
            notes.push(w)
        })?;
        for note in notes {
            warnings.push(json!({ "set": set, "token": name, "note": note }));
        }
        let (ty, value, description) = match converted {
            Converted::Token {
                ty,
                value,
                description,
            } => (ty, value, description),
            Converted::Unsupported(reason) => {
                unsupported.push(js::object(vec![
                    ("set", Some(set.into())),
                    ("token", Some(name.into())),
                    ("type", pt.ty.clone()),
                    ("reason", Some(reason.into())),
                ]));
                continue;
            }
        };
        if let Some(c) = c {
            if same(&c.value, &value)
                && js::strict_eq(c.ty.as_ref(), Some(&ty))
                && same_description(c.description.as_ref(), description.as_ref())
            {
                continue;
            }
        }
        set_token(&mut tree, name, Some(&ty), &value, description.as_ref());
        match c {
            Some(c) => diff
                .changed
                .push(json!({ "token": name, "type": ty, "from": c.value, "to": value })),
            None => diff
                .added
                .push(json!({ "token": name, "type": ty, "value": value })),
        }
    }
    for (name, c) in cur.iter() {
        if incoming.has(name) {
            continue;
        }
        // Never exported (reported at export): kept without a word.
        if token_to_penpot_quiet(name, c).unsupported().is_some() {
            continue;
        }
        let entry = |with_value: bool| {
            js::object(vec![
                ("token", Some(name.into())),
                ("type", c.ty.clone()),
                ("value", with_value.then(|| c.value.clone())),
            ])
        };
        if prune {
            delete_token(&mut tree, name);
            diff.removed.push(entry(true));
        } else {
            diff.kept.push(entry(false));
        }
    }
    Ok(Merged {
        tree: js::key_order(tree),
        diff,
        unsupported,
        warnings,
    })
}
