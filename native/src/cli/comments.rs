//! The `comments` command and its subcommands, `comments wait` included.

use serde_json::Value;

use super::flag_key;
use super::word::{command, sub};
use super::{Ctx, Handler};
use crate::core::error::DfwError;
use crate::core::host::named_host;
use crate::core::js;
use crate::core::util::{out, to_json_pretty, ArgValue};
use crate::review::comments::{add_comment, all_comments, new_session, update_comment, Update};
use crate::review::inbox::{
    format_for_claude, in_background, take_undelivered, undelivered, wait_for_comments,
    WaitOptions, WAIT_COMMAND, WAIT_SECONDS,
};
use crate::review::status::{self, field, Status};

use crate::workflow::state::arg_truthy;

/// The host's words for a background command when `--host` or `DFW_HOST` names the host.
fn background(ctx: &Ctx) -> Result<Option<&'static str>, DfwError> {
    Ok(named_host(ctx.args.get(flag_key::HOST))?.and_then(|h| h.background()))
}

/// The commands of this group: every subcommand runs here.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![(command::COMMENTS, comments as Handler)]
}

fn print_json(value: &Value) -> Result<i32, DfwError> {
    out(&to_json_pretty(value));
    Ok(0)
}

fn comments(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let sub = a.pos(1).filter(|s| !s.is_empty()).unwrap_or(sub::LIST);
    let id = a.pos(2);
    let msg = arg_truthy(a, "message").or_else(|| arg_truthy(a, "m"));
    let by = |fallback: &str| arg_truthy(a, "by").or_else(|| Some(fallback.into()));
    let change = |update: Update| print_json(&update_comment(&root, id, &update)?);
    let to = |status: &str| Some(status.to_string());
    let takes_id = [
        "show", "ack", "fixed", "reply", "dismiss", "resolve", "reopen",
    ];
    if takes_id.contains(&sub) && id.is_none_or(str::is_empty) {
        return Err(DfwError::new(format!(
            "\"dfw comments {sub}\" needs a comment id: dfw comments {sub} <id> (\"dfw comments list\" shows the ids)."
        )));
    }
    match sub {
        "wait" => wait(ctx),
        sub::LIST => list(ctx),
        "show" => {
            let found = all_comments(&root)?
                .into_iter()
                .find(|c| id.is_some() && c.get(field::ID).and_then(Value::as_str) == id);
            match found {
                Some(c) => print_json(&c),
                None => Err(DfwError::new(format!(
                    "Comment {} not found.",
                    id.unwrap_or_default()
                ))),
            }
        }
        "ack" => change(Update {
            status: to(Status::Acknowledged.as_str()),
            message: msg,
            ..Update::default()
        }),
        "fixed" => change(Update {
            status: to(Status::Fixed.as_str()),
            message: msg,
            evidence: arg_truthy(a, "evidence"),
            ..Update::default()
        }),
        "reply" => match msg {
            None => Err(DfwError::new("--message is required.")),
            message => change(Update {
                message,
                ..Update::default()
            }),
        },
        "dismiss" => match msg {
            None => Err(DfwError::new(
                "Dismissing needs --message explaining why (and the user must agree).",
            )),
            message => change(Update {
                status: to(Status::Dismissed.as_str()),
                message,
                by: by("claude"),
                ..Update::default()
            }),
        },
        "resolve" => change(Update {
            status: to(Status::Resolved.as_str()),
            message: msg.or_else(|| Some("Confirmed by user.".into())),
            by: by("claude"),
            ..Update::default()
        }),
        "reopen" => change(Update {
            status: to(Status::Reopened.as_str()),
            message: msg.or_else(|| Some("Reopened.".into())),
            by: by("user"),
            ..Update::default()
        }),
        sub::ADD => {
            // `JSON.parse(a.json)`: the flag alone is `true`, a missing flag is not JSON.
            let input: Value = match a.get(flag_key::JSON) {
                Some(ArgValue::Flag) => Value::Bool(true),
                Some(ArgValue::Text(text)) => serde_json::from_str(text)?,
                None => return Err(DfwError::internal("\"undefined\" is not valid JSON")),
            };
            print_json(&add_comment(&root, &input)?)
        }
        "new-round" => print_json(&new_session(&root, None)?.session),
        "inbox" => {
            let list = if a.truthy("peek") {
                undelivered(&root)?
            } else {
                take_undelivered(&root, false)?
            };
            if list.is_empty() {
                out("No new comments.");
            } else {
                out(&format_for_claude(&list, background(ctx)?));
            }
            Ok(0)
        }
        other => Err(DfwError::new(format!(
            "Unknown comments subcommand \"{other}\"."
        ))),
    }
}

/// `dfw comments list [--status active|<s1,s2>] [--json]`.
fn list(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let mut list = all_comments(&ctx.root())?;
    let status_of = |c: &Value| {
        c.get(field::STATUS)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if let Some(filter) = arg_truthy(a, "status") {
        let filter = js::to_string(Some(&filter));
        if filter == "active" {
            list.retain(|c| status::of(c).is_some_and(Status::is_active));
        } else {
            let wanted: Vec<&str> = filter.split(',').collect();
            list.retain(|c| status_of(c).is_some_and(|s| wanted.contains(&s.as_str())));
        }
    }
    if a.truthy("json") {
        return print_json(&Value::Array(list));
    }
    if list.is_empty() {
        out("No comments.");
        return Ok(0);
    }
    let lines: Vec<String> = list
        .iter()
        .map(|c| {
            let place: Vec<String> = ["screen", "variant", "breakpoint"]
                .iter()
                .filter_map(|k| js::truthy_value(c.get(k)))
                .map(|v| js::to_string(Some(v)))
                .collect();
            let of_target = |k: &str| js::truthy_value(c.get(field::TARGET).and_then(|t| t.get(k)));
            let target = of_target(field::DFW_ID)
                .or(of_target(field::SELECTOR))
                .map_or("?".to_string(), |t| js::to_string(Some(t)));
            let source = of_target(field::SOURCE)
                .map_or(String::new(), |s| format!(" ({})", js::to_string(Some(s))));
            let last = c
                .get(field::THREAD)
                .and_then(Value::as_array)
                .and_then(|t| t.last());
            let last = last.map_or(String::new(), |l| {
                format!(
                    "\n    last: {}: {}",
                    js::to_string(l.get(field::BY)),
                    js::to_string(l.get(field::MESSAGE))
                )
            });
            format!(
                "{} [{}] {} -> {target}{source}\n    \"{}\"{last}",
                js::to_string(c.get(field::ID)),
                js::to_string(c.get(field::STATUS)),
                place.join("/"),
                js::to_string(c.get(field::COMMENT)),
            )
        })
        .collect();
    out(&lines.join("\n"));
    Ok(0)
}

/// `dfw comments wait [--timeout <seconds>]`: block until the user posts comments.
fn wait(ctx: &Ctx) -> Result<i32, DfwError> {
    // `(Number(a.timeout) || 3600) * 1000`; `--timeout` alone never gets here
    // (`require_flag_values`).
    let seconds = ctx.args.text("timeout").map_or(f64::NAN, js::str_to_number);
    let seconds = Some(seconds)
        .filter(|s| *s != 0.0 && !s.is_nan())
        .unwrap_or(WAIT_SECONDS);
    let options = WaitOptions {
        timeout_ms: seconds * 1000.0,
        ..WaitOptions::default()
    };
    // Before the wait, so a wrong DFW_HOST fails at once, not after an hour.
    let background = background(ctx)?;
    let list = wait_for_comments(&ctx.root(), options)?;
    if list.is_empty() {
        out(&format!(
            "No new comments before the timeout. Start \"{WAIT_COMMAND}\" again {} to keep listening.",
            in_background(background)
        ));
    } else {
        out(&format_for_claude(&list, background));
    }
    Ok(0)
}
