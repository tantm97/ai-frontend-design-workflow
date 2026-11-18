//! Ports `core/lib/pixeldiff.mjs`: pixel diff of screenshots ("dfw diff").
//!
//! Node ran pixelmatch (Mapbox, ISC, vendored in assets/vendor/pixelmatch.js, v8.0.0) inside the
//! headless browser, which also decoded and encoded the PNGs. Here the PNGs are read and written
//! with the `png` crate and the comparison is a port of pixelmatch's default path, so no browser
//! is needed. The page drew both images on white canvases of the larger size, which made every
//! pixel opaque: only pixelmatch's opaque colour path is ported, and images are flattened on
//! white the way the canvas did it.
//! Uses: what changed between two approved design versions (after "dfw reopen"), and where an
//! implementation screenshot departs from its design at G4. A number for triage, not a pass/fail:
//! real data and font rendering always differ a little.
//!
//! A few lines over 800 with its tests.

use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::util::js_number;

/// Default `threshold` of "dfw diff": the largest OKLab HyAB distance still counted as equal.
pub const THRESHOLD: f64 = 0.1;

/// Two PNG files to compare and where the diff image goes (`{ a, b, out }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub a: PathBuf,
    pub b: PathBuf,
    pub out: Option<PathBuf>,
}

/// What differs between the two images of a pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    /// Pixels that differ (anti-aliasing excluded).
    pub diff: u64,
    pub w: u32,
    pub h: u32,
    /// `diff / (w * h)`.
    pub ratio: f64,
    /// "WxH -> WxH" when the two images are not the same size.
    pub size_changed: Option<String>,
}

/// One entry of the result of [`diff_images`]: the pair and its diff, or why it failed.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffResult {
    pub pair: Pair,
    pub outcome: Result<Diff, String>,
}

impl DiffResult {
    /// The entry as Node returned it: `{ a, b, out, diff, w, h, ratio, sizeChanged }` or
    /// `{ a, b, out, error }`.
    pub fn to_value(&self) -> Value {
        let mut o = Map::new();
        o.insert("a".into(), json!(self.pair.a.display().to_string()));
        o.insert("b".into(), json!(self.pair.b.display().to_string()));
        if let Some(out) = &self.pair.out {
            o.insert("out".into(), json!(out.display().to_string()));
        }
        match &self.outcome {
            Ok(d) => {
                o.insert("diff".into(), json!(d.diff));
                o.insert("w".into(), json!(d.w));
                o.insert("h".into(), json!(d.h));
                o.insert("ratio".into(), json!(d.ratio));
                o.insert("sizeChanged".into(), json!(d.size_changed));
            }
            Err(error) => {
                o.insert("error".into(), json!(error));
            }
        }
        Value::Object(o)
    }
}

/// A decoded image: 8-bit RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Read a PNG of any colour type as 8-bit RGBA.
pub fn decode_rgba(file: &Path) -> Result<Image, String> {
    let bytes = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    // What the page's `img.onerror` rejected with, as Node printed it (without the page's stack line).
    decode(&bytes).ok_or_else(|| "Error: cannot decode image".to_string())
}

fn decode(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        // normalize_to_color8 expands a palette to RGB or RGBA.
        png::ColorType::Indexed => return None,
    };
    let (w, h) = (info.width, info.height);
    (rgba.len() == w as usize * h as usize * 4).then_some(Image { w, h, rgba })
}

/// Write 8-bit RGBA as a PNG (folders are created).
fn encode(file: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut encoder = png::Encoder::new(BufWriter::new(fs::File::create(file)?), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()?;
        Ok(())
    };
    write().map_err(|e| format!("{}: {e}", file.display()))
}

/// The image drawn at the top left of a white `w` x `h` canvas (`pixels(img)` in the page):
/// the canvas premultiplies, so a channel is `round(c * a / 255) + 255 - a`.
fn on_white(image: &Image, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![255u8; w as usize * h as usize * 4];
    let row = image.w as usize * 4;
    for (y, line) in image.rgba.chunks_exact(row.max(1)).enumerate() {
        let start = y * w as usize * 4;
        for (dst, src) in out[start..start + row]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(line.as_chunks::<4>().0)
        {
            let a = u32::from(src[3]);
            for c in 0..3 {
                dst[c] = ((u32::from(src[c]) * a + 127) / 255 + 255 - a) as u8;
            }
        }
    }
    out
}

/// `diffImages(pairs, { threshold })`: compare every pair and write its diff image (red =
/// changed, yellow = anti-aliasing, the rest a faded copy of the first image).
pub fn diff_images(pairs: &[Pair], threshold: f64) -> Vec<DiffResult> {
    let tables = Tables::new();
    pairs
        .iter()
        .map(|pair| DiffResult {
            pair: pair.clone(),
            outcome: diff_pair(pair, threshold, &tables),
        })
        .collect()
}

fn diff_pair(pair: &Pair, threshold: f64, tables: &Tables) -> Result<Diff, String> {
    let (ia, ib) = (decode_rgba(&pair.a)?, decode_rgba(&pair.b)?);
    let (w, h) = (ia.w.max(ib.w), ia.h.max(ib.h));
    let (da, db) = (on_white(&ia, w, h), on_white(&ib, w, h));
    let mut out = vec![0u8; da.len()];
    let diff = pixelmatch(
        &da, &db, &mut out, w as usize, h as usize, threshold, tables,
    );
    if let Some(file) = &pair.out {
        encode(file, w, h, &out)?;
    }
    let size = |i: &Image| format!("{}x{}", i.w, i.h);
    Ok(Diff {
        diff,
        w,
        h,
        ratio: diff as f64 / (f64::from(w) * f64::from(h)),
        size_changed: (size(&ia) != size(&ib)).then(|| format!("{} -> {}", size(&ia), size(&ib))),
    })
}

/// `diffPairs(a, b, out)`: pairs for "dfw diff <a> <b> [--out]": two files, or two folders
/// matched by file name.
pub fn diff_pairs(a: &Path, b: &Path, out: Option<&Path>) -> Result<Vec<Pair>, DfwError> {
    if a.is_dir() && b.is_dir() {
        let out_dir = out.map_or_else(|| b.join("_diff"), Path::to_path_buf);
        let pairs: Vec<Pair> = fs::read_dir(a)?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.ends_with(".png") && b.join(name).exists())
            .map(|name| Pair {
                a: a.join(&name),
                b: b.join(&name),
                out: Some(out_dir.join(&name)),
            })
            .collect();
        if pairs.is_empty() {
            return Err(DfwError::new(format!(
                "No PNG file names in common between {} and {}.",
                a.display(),
                b.display()
            )));
        }
        return Ok(pairs);
    }
    for f in [a, b] {
        if !f.exists() {
            return Err(DfwError::new(format!("Not found: {}", f.display())));
        }
    }
    let default_out = || {
        // b.replace(/(\.png)?$/i, '.diff.png')
        let text = b.display().to_string();
        let stem = match text.len().checked_sub(4) {
            Some(at) if text.is_char_boundary(at) && text[at..].eq_ignore_ascii_case(".png") => {
                &text[..at]
            }
            _ => &text[..],
        };
        PathBuf::from(format!("{stem}.diff.png"))
    };
    Ok(vec![Pair {
        a: a.to_path_buf(),
        b: b.to_path_buf(),
        out: Some(out.map_or_else(default_out, Path::to_path_buf)),
    }])
}

/// One line of "dfw diff" for a result: `  1.25%  name.png  (size ...)  -> out` or
/// `ERROR  name.png: why`. `out` is the diff image as the caller wants it shown.
pub fn format_line(result: &DiffResult, out: &str) -> String {
    let name = result
        .pair
        .b
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match &result.outcome {
        Err(error) => format!("ERROR  {name}: {error}"),
        Ok(d) => {
            let size = d
                .size_changed
                .as_ref()
                .map(|s| format!("  (size {s})"))
                .unwrap_or_default();
            format!("{:>6}%  {name}{size}  -> {out}", to_fixed2(d.ratio * 100.0))
        }
    }
}

/// `x.toFixed(2)` for a finite, non-negative number below 1e21: the exact decimal value rounded
/// half up (Rust's own formatting rounds an exact half to even).
fn to_fixed2(x: f64) -> String {
    if !x.is_finite() || x < 0.0 {
        return js_number(x);
    }
    // 60 decimals show a double of this size exactly enough to see which way the third goes.
    let exact = format!("{x:.60}");
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, "0"));
    let mut cents: u128 = format!("{int}{}", &frac[..2]).parse().unwrap_or(0);
    if frac.as_bytes()[2] >= b'5' {
        cents += 1;
    }
    format!("{}.{:02}", cents / 100, cents % 100)
}

/* ---------------------------------------------------------------- pixelmatch v8.0.0 */

/// The lookup tables of pixelmatch.
struct Tables {
    /// Premultiplied LMS matrix contributions for sRGB byte values: [L, M, S] x [R, G, B].
    lms: [[[f64; 256]; 3]; 3],
    /// Cube root over [0..1], 4096 steps, for linear interpolation.
    cbrt: Vec<f64>,
}

const CBRT_N: usize = 4096;
const CBRT_EXACT: usize = 8;
const TOE_K1: f64 = 0.206;
const TOE_K2: f64 = 0.03;
const TOE_K3: f64 = (1.0 + TOE_K1) / (1.0 + TOE_K2);
const LMS: [[f64; 3]; 3] = [
    [0.4122214708, 0.5363325363, 0.0514459929],
    [0.2119034982, 0.6806995451, 0.1073969566],
    [0.0883024619, 0.2817188376, 0.6299787005],
];

impl Tables {
    fn new() -> Tables {
        let mut lms = [[[0.0; 256]; 3]; 3];
        for i in 0..256 {
            let c = i as f64 / 255.0;
            // sRGB [0..255] -> linear [0..1]
            let lin = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            for (row, coefficients) in lms.iter_mut().zip(LMS) {
                for (channel, k) in row.iter_mut().zip(coefficients) {
                    channel[i] = k * lin;
                }
            }
        }
        let cbrt = (0..=CBRT_N + 1)
            .map(|i| (i as f64 / CBRT_N as f64).cbrt())
            .collect();
        Tables { lms, cbrt }
    }

    /// `cbrtLUT(x)`.
    fn cbrt(&self, x: f64) -> f64 {
        let t = x * CBRT_N as f64;
        let i = t as usize;
        // cbrt curves hardest right above zero, where interpolation is far off.
        if i < CBRT_EXACT {
            return x.cbrt();
        }
        self.cbrt[i] + (self.cbrt[i + 1] - self.cbrt[i]) * (t - i as f64)
    }

    /// Cube-rooted LMS and toe-corrected OKLab lightness of an opaque sRGB pixel.
    fn oklab(&self, p: &[u8]) -> [f64; 4] {
        let (r, g, b) = (p[0] as usize, p[1] as usize, p[2] as usize);
        let mix = |row: &[[f64; 256]; 3]| self.cbrt(row[0][r] + row[1][g] + row[2][b]);
        let (l, m, s) = (mix(&self.lms[0]), mix(&self.lms[1]), mix(&self.lms[2]));
        [l, m, s, lightness(l, m, s)]
    }

    /// `colorDeltaOpaque`: 0 when the HyAB OKLab distance of two different opaque pixels stays
    /// within `max_delta`, else 1, or -1 when the second pixel is darker.
    fn color_delta(&self, p1: &[u8], p2: &[u8], max_delta: f64) -> i8 {
        let [l1, m1, s1, lr1] = self.oklab(p1);
        let [l2, m2, s2, lr2] = self.oklab(p2);
        let (d_lr, dl, dm, ds) = (lr1 - lr2, l1 - l2, m1 - m2, s1 - s2);
        // |dLr| + sqrt(da^2 + db^2) against max_delta, without the sqrt.
        let rest = max_delta - d_lr.abs();
        if rest >= 0.0 {
            let da = 1.9779984951 * dl - 2.4285922050 * dm + 0.4505937099 * ds;
            let db = 0.0259040371 * dl + 0.7827717662 * dm - 0.8086757660 * ds;
            if da * da + db * db <= rest * rest {
                return 0;
            }
        }
        if d_lr > 0.0 {
            -1
        } else {
            1
        }
    }
}

/// Toe-corrected OKLab lightness (Lr) from cube-rooted LMS values.
fn lightness(l: f64, m: f64, s: f64) -> f64 {
    let big_l = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let x = TOE_K3 * big_l - TOE_K1;
    0.5 * (x + (x * x + 4.0 * TOE_K2 * TOE_K3 * big_l).sqrt())
}

/// `pixelmatch(img1, img2, output, width, height, { threshold })` with its defaults, for opaque
/// images. Returns the number of mismatched pixels and draws the diff into `output`.
fn pixelmatch(
    img1: &[u8],
    img2: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    threshold: f64,
    tables: &Tables,
) -> u64 {
    const ALPHA: f64 = 0.1;
    let len = width * height;
    let px = |img: &[u8], i: usize| -> [u8; 4] {
        [img[i * 4], img[i * 4 + 1], img[i * 4 + 2], img[i * 4 + 3]]
    };
    if img1 == img2 {
        for i in 0..len {
            draw_gray_pixel(output, i * 4, img1, ALPHA);
        }
        return 0;
    }
    let mut diff = 0;
    for i in 0..len {
        let pos = i * 4;
        let delta = if px(img1, i) == px(img2, i) {
            0
        } else {
            tables.color_delta(&img1[pos..pos + 4], &img2[pos..pos + 4], threshold)
        };
        if delta == 0 {
            // Similar: the first image in grey, blended with white.
            draw_gray_pixel(output, pos, img1, ALPHA);
            continue;
        }
        let (x, y) = (i % width, i / width);
        // A real rendering difference, or just anti-aliasing?
        if antialiased(img1, x, y, width, height, img1, img2)
            || antialiased(img2, x, y, width, height, img2, img1)
        {
            draw_pixel(output, pos, [255, 255, 0]);
        } else {
            draw_pixel(output, pos, [255, 0, 0]);
            diff += 1;
        }
    }
    diff
}

fn same(img: &[u8], i: usize, j: usize) -> bool {
    img[i * 4..i * 4 + 4] == img[j * 4..j * 4 + 4]
}

/// Whether a pixel is likely part of anti-aliasing ("Anti-aliased Pixel and Intensity Slope
/// Detector", V. Vysniauskas, 2009).
fn antialiased(
    img: &[u8],
    x1: usize,
    y1: usize,
    width: usize,
    height: usize,
    a: &[u8],
    b: &[u8],
) -> bool {
    let x0 = x1.saturating_sub(1);
    let y0 = y1.saturating_sub(1);
    let x2 = (x1 + 1).min(width - 1);
    let y2 = (y1 + 1).min(height - 1);
    let center = (y1 * width + x1) * 4;
    let mut zeroes = usize::from(x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2);
    let (mut min, mut max) = (0.0, 0.0);
    let (mut min_at, mut max_at) = ((0, 0), (0, 0));
    // The 8 adjacent pixels.
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            // Brightness delta between the centre pixel and this one.
            let delta = brightness_delta(img, center, (y * width + x) * 4);
            if delta == 0.0 {
                zeroes += 1;
                // More than 2 equal siblings: definitely not anti-aliasing.
                if zeroes > 2 {
                    return false;
                }
            } else if delta < min {
                min = delta;
                min_at = (x, y);
            } else if delta > max {
                max = delta;
                max_at = (x, y);
            }
        }
    }
    // No darker and brighter pixels among the siblings: not anti-aliasing.
    if min == 0.0 || max == 0.0 {
        return false;
    }
    // If the darkest or the brightest pixel has 3+ equal siblings in both images (definitely
    // not anti-aliased), this pixel is anti-aliased.
    let many = |at: (usize, usize)| {
        has_many_siblings(a, at.0, at.1, width, height)
            && has_many_siblings(b, at.0, at.1, width, height)
    };
    many(min_at) || many(max_at)
}

/// Whether a pixel has 3+ adjacent pixels of the same colour (the image border counts as one).
fn has_many_siblings(img: &[u8], x1: usize, y1: usize, width: usize, height: usize) -> bool {
    let x0 = x1.saturating_sub(1);
    let y0 = y1.saturating_sub(1);
    let x2 = (x1 + 1).min(width - 1);
    let y2 = (y1 + 1).min(height - 1);
    let pos = y1 * width + x1;
    let mut zeroes = usize::from(x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2);
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            zeroes += usize::from(same(img, pos, y * width + x));
            if zeroes > 2 {
                return true;
            }
        }
    }
    false
}

/// `brightnessDelta` for opaque pixels: gamma-space Rec.601 luma of the centre minus the
/// neighbour's.
fn brightness_delta(img: &[u8], center: usize, m: usize) -> f64 {
    let d = |c: usize| f64::from(img[center + c]) - f64::from(img[m + c]);
    let (dr, dg, db) = (d(0), d(1), d(2));
    if dr == 0.0 && dg == 0.0 && db == 0.0 {
        return 0.0;
    }
    dr * 0.29889531 + dg * 0.58662247 + db * 0.11448223
}

fn draw_pixel(output: &mut [u8], pos: usize, rgb: [u8; 3]) {
    output[pos..pos + 3].copy_from_slice(&rgb);
    output[pos + 3] = 255;
}

fn draw_gray_pixel(output: &mut [u8], i: usize, img: &[u8], alpha: f64) {
    let luma = f64::from(img[i]) * 0.29889531
        + f64::from(img[i + 1]) * 0.58662247
        + f64::from(img[i + 2]) * 0.11448223;
    let val = 255.0 + (luma - 255.0) * alpha * f64::from(img[i + 3]) / 255.0;
    // A Uint8ClampedArray rounds an exact half to even.
    let val = val.clamp(0.0, 255.0).round_ties_even() as u8;
    draw_pixel(output, i, [val, val, val]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat((w * h) as usize)
    }

    fn write(file: &Path, w: u32, h: u32, rgba: &[u8]) {
        encode(file, w, h, rgba).unwrap();
    }

    /// diff: identical screenshots are 0%, a changed one is not (needs Chrome)
    /// (no browser here: the images are made in the test)
    #[test]
    fn identical_images_are_zero_a_changed_one_is_not() {
        let dir = test_dir("diff");
        let mut text = solid(200, 100, [255, 255, 255, 255]);
        for y in 20..40 {
            for x in 20..120 {
                let at = (y * 200 + x) * 4;
                text[at..at + 3].copy_from_slice(&[20, 20, 20]);
            }
        }
        let mut red = text.clone();
        for p in red
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .filter(|p| p[0] == 255)
        {
            *p = [0xd0, 0x30, 0x30, 0xff];
        }
        write(&dir.join("a.png"), 200, 100, &text);
        write(&dir.join("b.png"), 200, 100, &text);
        write(&dir.join("c.png"), 200, 100, &red);
        let pair = |b: &str, out: Option<&str>| Pair {
            a: dir.join("a.png"),
            b: dir.join(b),
            out: out.map(|o| dir.join(o)),
        };
        let results = diff_images(
            &[pair("b.png", None), pair("c.png", Some("x/d.png"))],
            THRESHOLD,
        );
        let same = results[0].outcome.as_ref().unwrap();
        assert_eq!((same.diff, same.ratio, &same.size_changed), (0, 0.0, &None));
        let changed = results[1].outcome.as_ref().unwrap();
        assert_eq!(changed.diff, 200 * 100 - 100 * 20);
        assert!(changed.ratio > 0.5, "changed ratio {}", changed.ratio);
        let d = decode_rgba(&dir.join("x/d.png")).unwrap();
        assert_eq!((d.w, d.h), (200, 100));
        assert_eq!(&d.rgba[..4], [255, 0, 0, 255], "changed pixels are red");
        let unchanged = (20 * 200 + 20) * 4;
        assert_eq!(
            &d.rgba[unchanged..unchanged + 4],
            [232, 232, 232, 255],
            "the rest is the first image, faded"
        );
        assert_eq!(
            results[1].to_value()["ratio"],
            json!(0.9),
            "{}",
            results[1].to_value()
        );
    }

    #[test]
    fn sizes_alpha_and_colour_types() {
        let dir = test_dir("diff-size");
        write(&dir.join("small.png"), 2, 2, &solid(2, 2, [0, 0, 0, 255]));
        write(&dir.join("big.png"), 3, 4, &solid(3, 4, [0, 0, 0, 255]));
        let r = diff_images(
            &[Pair {
                a: dir.join("small.png"),
                b: dir.join("big.png"),
                out: Some(dir.join("out.png")),
            }],
            THRESHOLD,
        );
        let d = r[0].outcome.as_ref().unwrap();
        // The smaller image is padded with white: 12 - 4 pixels differ.
        assert_eq!((d.w, d.h, d.diff), (3, 4, 8));
        assert_eq!(d.size_changed.as_deref(), Some("2x2 -> 3x4"));
        assert_eq!(
            format_line(&r[0], "out.png"),
            " 66.67%  big.png  (size 2x2 -> 3x4)  -> out.png"
        );

        // Half-transparent black on white is mid grey; fully transparent is white.
        let flat = on_white(
            &Image {
                w: 2,
                h: 1,
                rgba: vec![0, 0, 0, 128, 10, 20, 30, 0],
            },
            2,
            1,
        );
        assert_eq!(flat, [127, 127, 127, 255, 255, 255, 255, 255]);

        // An 8-bit greyscale PNG decodes to RGBA.
        let gray = dir.join("gray.png");
        let mut encoder = png::Encoder::new(fs::File::create(&gray).unwrap(), 2, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0, 200]).unwrap();
        writer.finish().unwrap();
        assert_eq!(
            decode_rgba(&gray).unwrap().rgba,
            [0, 0, 0, 255, 200, 200, 200, 255]
        );

        fs::write(dir.join("broken.png"), b"not a png").unwrap();
        let broken = diff_images(
            &[Pair {
                a: dir.join("broken.png"),
                b: dir.join("big.png"),
                out: None,
            }],
            THRESHOLD,
        );
        assert_eq!(
            broken[0].outcome,
            Err("Error: cannot decode image".to_string())
        );
        assert_eq!(
            format_line(&broken[0], ""),
            "ERROR  big.png: Error: cannot decode image"
        );
        assert_eq!(
            broken[0].to_value()["error"],
            json!("Error: cannot decode image")
        );
    }

    #[test]
    fn small_colour_changes_stay_under_the_threshold_and_antialiasing_is_not_counted() {
        let tables = Tables::new();
        assert_eq!(
            tables.color_delta(&[200, 200, 200, 255], &[203, 200, 199, 255], 0.1),
            0
        );
        assert_eq!(
            tables.color_delta(&[255, 255, 255, 255], &[0, 0, 0, 255], 0.1),
            -1
        );
        assert_eq!(
            tables.color_delta(&[0, 0, 0, 255], &[255, 255, 255, 255], 0.1),
            1
        );
        // Black against white is the whole scale: just under 1 passes, 1 does not differ.
        assert_eq!(
            tables.color_delta(&[0, 0, 0, 255], &[255, 255, 255, 255], 1.01),
            0
        );

        // A diagonal edge moved by one pixel, with a grey ramp pixel on the edge: the ramp is
        // anti-aliasing (yellow, not counted).
        let (w, h) = (8usize, 8usize);
        let edge = |shift: usize| {
            let mut img = solid(w as u32, h as u32, [255, 255, 255, 255]);
            for y in 0..h {
                for x in 0..w {
                    let at = (y * w + x) * 4;
                    let v = match (x + shift).cmp(&y) {
                        std::cmp::Ordering::Less => 0,
                        std::cmp::Ordering::Equal => 128,
                        std::cmp::Ordering::Greater => 255,
                    };
                    img[at..at + 3].copy_from_slice(&[v, v, v]);
                }
            }
            img
        };
        let (a, b) = (edge(0), edge(1));
        let mut out = vec![0; a.len()];
        let diff = pixelmatch(&a, &b, &mut out, w, h, 0.1, &tables);
        let yellow = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..3] == [255, 255, 0])
            .count();
        let red = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..3] == [255, 0, 0])
            .count();
        assert_eq!(diff as usize, red);
        assert!(yellow > 0, "anti-aliased pixels are drawn yellow");
        assert!(out.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn pairs_for_files_and_folders() {
        let dir = test_dir("diff-pairs");
        let (a, b) = (dir.join("a"), dir.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for f in [
            "a/one.png",
            "a/two.png",
            "a/notes.txt",
            "b/one.png",
            "b/notes.txt",
            "b/three.PNG",
        ] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let pairs = diff_pairs(&a, &b, None).unwrap();
        assert_eq!(
            pairs,
            [Pair {
                a: a.join("one.png"),
                b: b.join("one.png"),
                out: Some(b.join("_diff").join("one.png"))
            }]
        );
        let out = dir.join("o");
        assert_eq!(
            diff_pairs(&a, &b, Some(&out)).unwrap()[0].out,
            Some(out.join("one.png"))
        );
        let empty = dir.join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(
            diff_pairs(&a, &empty, None).unwrap_err().message(),
            format!(
                "No PNG file names in common between {} and {}.",
                a.display(),
                empty.display()
            )
        );
        let file = |b: &Path| {
            diff_pairs(&a.join("one.png"), b, None).unwrap()[0]
                .out
                .clone()
                .unwrap()
        };
        assert_eq!(file(&b.join("one.png")), b.join("one.diff.png"));
        assert_eq!(file(&b.join("three.PNG")), b.join("three.diff.png"));
        assert_eq!(file(&b.join("notes.txt")), b.join("notes.txt.diff.png"));
        assert_eq!(
            diff_pairs(&a.join("one.png"), &b.join("gone.png"), None)
                .unwrap_err()
                .message(),
            format!("Not found: {}", b.join("gone.png").display())
        );
    }

    #[test]
    fn to_fixed_rounds_like_javascript() {
        // (0.125).toFixed(2) is "0.13" in JavaScript; Rust's {:.2} gives 0.12.
        for (x, text) in [
            (0.125, "0.13"),
            (0.0, "0.00"),
            (66.666_666, "66.67"),
            (100.0, "100.00"),
            (0.005, "0.01"),
            (1.005, "1.00"),
            (0.375, "0.38"),
            (99.995, "100.00"),
            (12.344_999, "12.34"),
        ] {
            assert_eq!(to_fixed2(x), text, "{x}");
        }
    }
}
