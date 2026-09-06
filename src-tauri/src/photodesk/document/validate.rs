//! Consistency rules serde cannot express.
//!
//! Everything here is a rule a *well-formed* document can still break. Types catch
//! "this key is not in the schema"; this file catches "this document contradicts
//! itself", which is the class serde has no opinion about.
//!
//! Each rule below carries the sentence that justifies it, and the list is
//! deliberately short. §6.1 states no parameter ranges and §11 puts slider travel in
//! the UI, so inventing bounds here would put numbers in the register's blind spot —
//! the rules that exist are the ones where a violating document is *incoherent*, not
//! merely unusual.

use super::schema::{Document, Layer, MetadataPolicy, Output, OutputFormat, Source};

pub fn check(doc: &Document) -> Result<(), String> {
    check_source(&doc.source)?;
    check_geometry(doc)?;
    check_stack(&doc.stack)?;
    check_output(&doc.output)?;
    no_cache_paths(doc)?;
    Ok(())
}

fn check_source(source: &Source) -> Result<(), String> {
    // §12.3 re-hashes the source before and after every cycle; a hash it cannot parse
    // is a hash it cannot compare, and the test silently becomes a no-op.
    let Some(hex) = source.hash.strip_prefix("blake3:") else {
        return Err(format!(
            "source.hash `{}` names no algorithm. §12.3 compares this value across a \
             full open/edit/export cycle and has to know what it is comparing",
            source.hash
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "source.hash is not 64 hex digits of blake3: `{hex}`"
        ));
    }

    // A photograph and its sidecar move together. An absolute path survives none of
    // the journeys they actually make, and `..` in a document is a file reference
    // that reaches outside the directory the user opened.
    let file = &source.file;
    if file.is_empty() {
        return Err("source.file is empty".into());
    }
    if file.starts_with('/') || file.contains(':') {
        return Err(format!(
            "source.file `{file}` is absolute. It is resolved against the sidecar's own \
             directory, so an absolute path stops being true the first time the photo \
             is copied anywhere"
        ));
    }
    if file.split('/').any(|part| part == "..") {
        return Err(format!(
            "source.file `{file}` escapes its own directory. A document may name the \
             photograph beside it and nothing else"
        ));
    }

    if source.dimensions[0] == 0 || source.dimensions[1] == 0 {
        return Err(format!(
            "source.dimensions {:?} has a zero side",
            source.dimensions
        ));
    }
    if !(1..=8).contains(&source.orientation) {
        return Err(format!(
            "source.orientation {} is not an EXIF orientation (1–8)",
            source.orientation
        ));
    }
    Ok(())
}

fn check_geometry(doc: &Document) -> Result<(), String> {
    let Some(crop) = doc.geometry.crop else {
        return Ok(());
    };
    // Normalised source coordinates, so the crop survives a proxy change (§7.1). A
    // crop outside the unit square describes pixels the source does not have.
    let fields = [("x", crop.x), ("y", crop.y), ("w", crop.w), ("h", crop.h)];
    for (name, v) in fields {
        if !v.is_finite() {
            return Err(format!("geometry.crop.{name} is not a finite number"));
        }
    }
    if crop.w <= 0.0 || crop.h <= 0.0 {
        return Err(format!(
            "geometry.crop is {}×{} — a crop with no area is not a crop",
            crop.w, crop.h
        ));
    }
    if crop.x < 0.0 || crop.y < 0.0 || crop.x + crop.w > 1.0 + 1e-6 || crop.y + crop.h > 1.0 + 1e-6
    {
        return Err(format!(
            "geometry.crop ({}, {}) {}×{} leaves the unit square; it is in normalised \
             source coordinates",
            crop.x, crop.y, crop.w, crop.h
        ));
    }
    if let Some(rotate) = doc.geometry.rotate
        && !rotate.is_finite()
    {
        return Err("geometry.rotate is not a finite number".into());
    }
    Ok(())
}

fn check_stack(stack: &[Layer]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::with_capacity(stack.len());
    for layer in stack {
        if layer.id.is_empty() {
            return Err("a layer has an empty id".into());
        }
        // §9.3 derives cache keys from the layer id and §12.2 seeds grain from it, so
        // two layers sharing an id share a cache entry and a grain pattern. The
        // symptom of that is one layer's noise appearing on another, which nobody
        // would trace back to a duplicate string in a sidecar.
        if seen.contains(&layer.id.as_str()) {
            return Err(format!(
                "two layers share the id `{}`. §9.3's cache keys and §12.2's grain seed \
                 both derive from it, so duplicates alias",
                layer.id
            ));
        }
        seen.push(&layer.id);

        // `op`/`op_version` and `params` are two fields that must move together. They
        // agree by construction on load; this catches a document assembled in memory.
        let (op, op_version) = layer.params.op();
        if op != layer.op || op_version != layer.op_version {
            return Err(format!(
                "layer `{}` declares {:?} v{} but carries {:?} v{} parameters",
                layer.id, layer.op, layer.op_version, op, op_version
            ));
        }

        // A NaN here reaches the working space, survives every later stage — NaN
        // propagates through arithmetic rather than being caught by it — and lands in
        // the exported file. JSON cannot represent one, so its only source is a writer
        // that was already wrong, and the cheapest place to stop it is here.
        if let Some(field) = layer.params.non_finite() {
            return Err(format!(
                "layer `{}` has a non-finite `{field}`",
                layer.id
            ));
        }

        if let Some(opacity) = layer.opacity
            && !(0.0..=1.0).contains(&opacity)
        {
            return Err(format!(
                "layer `{}` has opacity {opacity}, which is outside 0–1",
                layer.id
            ));
        }
    }
    Ok(())
}

fn check_output(output: &Output) -> Result<(), String> {
    match (output.format, output.quality) {
        // JPEG without a quality is an export that cannot be reproduced: the next
        // encoder picks its own default and "export again, same result" stops holding.
        (OutputFormat::Jpeg, None) => Err(
            "output.format is jpeg with no quality. Two exports of the same document \
             would then differ by whatever default the encoder happened to have"
                .into(),
        ),
        (_, Some(q)) if !(1..=100).contains(&q) => {
            Err(format!("output.quality {q} is outside 1–100"))
        }
        _ => Ok(()),
    }
}

/// §0, frozen: **cache paths are never written into the document.**
///
/// Checked over the serialised form rather than field by field, because the point of
/// the rule is that it holds for the *whole* document — a per-field check would have
/// to be extended every time the schema grows, and would be forgotten exactly once.
fn no_cache_paths(doc: &Document) -> Result<(), String> {
    fn walk(value: &serde_json::Value, path: &str) -> Result<(), String> {
        match value {
            serde_json::Value::String(s) => {
                if s.contains(".photodesk/") || s.starts_with(".photodesk") {
                    return Err(format!(
                        "{path} names something inside the cache directory: `{s}`. §0 \
                         freezes every cache as regenerable, so a document that points \
                         into one has a dangling reference by design"
                    ));
                }
                Ok(())
            }
            serde_json::Value::Array(items) => items
                .iter()
                .enumerate()
                .try_for_each(|(i, v)| walk(v, &format!("{path}[{i}]"))),
            serde_json::Value::Object(map) => map
                .iter()
                .try_for_each(|(k, v)| walk(v, &format!("{path}.{k}"))),
            _ => Ok(()),
        }
    }
    let value = serde_json::to_value(doc).map_err(|e| e.to_string())?;
    walk(&value, "document")
}

/// Not a rule — a reminder in code that `metadata` has a default and it is not `keep`.
///
/// §6.1 sets `keep-minus-gps`, and the reason is worth having next to the type: a
/// photograph exported to the internet with its GPS intact tells everyone where the
/// photographer's home is.
pub const DEFAULT_METADATA: MetadataPolicy = MetadataPolicy::KeepMinusGps;
