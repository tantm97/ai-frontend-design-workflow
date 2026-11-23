//! `String(number)`: a number printed the way JavaScript prints it. A leaf module (it uses nothing
//! of the crate), so `core::util` can format JSON numbers without depending on the rest of
//! `core::js`, which reads JSON through `core::util`.

/// `String(number)`, including `NaN` and `Infinity`.
pub fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        finite_number(v)
    }
}

/// `Number.prototype.toString()` of a finite number (ECMAScript Number::toString).
fn finite_number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    // `{:e}` gives the shortest digits that read back as the same double: "d.ddde<exp>".
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", e.abs())
        } else {
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    };
    if v < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}
