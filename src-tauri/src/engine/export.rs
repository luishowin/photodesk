//! A rendered frame becomes a file (§6.1's `output` block).
//!
//! ```json
//! "output": { "format": "jpeg", "quality": 92, "colorspace": "srgb", "metadata": "keep-minus-gps" }
//! ```
//!
//! Four things and each one is a decision the document already made. What this module
//! adds is the two that only become visible when bytes are written:
//!
//! **The file is ICC-tagged, always.** §4's chain ends "→ ICC-tagged file", and an
//! untagged export is the thing §4's own decode path has to guess about at the other
//! end — assuming sRGB for a file that says nothing is right, and producing such a file
//! when we know better is not.
//!
//! **The output is deterministic.** Exporting the same document twice produces the same
//! bytes: no timestamps in the profile, no encoder state that depends on when it ran.
//! §12.1's golden images cannot be blessed otherwise — a reference that differs from
//! the render by the second it was made in is a reference that fails every time.
//!
//! ## Not yet: tiling
//!
//! §7.1 says export is "tiled full-res" and this writes whole images. The renderer
//! holds one texture per live node, so a 12 MP export is a few hundred megabytes rather
//! than the gigabyte §7.3 caps at — comfortable, but not the streaming path §7.1
//! describes and not what a 60 MP file will want.

use crate::photodesk::document::{ColorSpace, MetadataPolicy, Output, OutputFormat};

use super::colour::{DISPLAY_P3, SRGB, Space};
use super::exif::{self, Exif};
use super::icc;
use super::render::Rendered;

#[derive(Debug)]
pub enum ExportError {
    /// A format the document may name and this build cannot write.
    ///
    /// Named rather than substituted. Silently writing a PNG where a document asked
    /// for a TIFF is the kind of helpfulness that turns into a support question.
    Unsupported { format: OutputFormat, since: &'static str },
    Encoder(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Unsupported { format, since } => write!(
                f,
                "this document asks for {format:?}, which PhotoDesk does not write yet \
                 ({since}). It is refused rather than substituted"
            ),
            ExportError::Encoder(e) => write!(f, "the encoder failed: {e}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// What the source file had to say for itself, for §6.1's `metadata` policy to act on.
///
/// Borrowed rather than owned by [`Rendered`] because the pixels and the metadata come
/// from different places — the metadata is the *source's*, and by the time a frame has
/// been rendered the source is long closed.
#[derive(Clone, Debug, Default)]
pub struct SourceMetadata {
    pub exif: Option<Exif>,
}

impl SourceMetadata {
    /// Read what a JPEG says about itself. HEIF metadata is a separate shape and
    /// arrives with the HEIF export path.
    pub fn from_jpeg(bytes: &[u8]) -> Self {
        Self { exif: exif::from_jpeg(bytes) }
    }
}

/// Encode a rendered frame under `output`.
pub fn encode(
    rendered: &Rendered,
    output: &Output,
    source: &SourceMetadata,
) -> Result<Vec<u8>, ExportError> {
    // The renderer already encoded into the document's output space (§5 stage 13), so
    // this asserts the two agree rather than converting between them. A mismatch here
    // would mean the graph and the output block disagree, which the compiler prevents
    // — this is the assertion that says so.
    debug_assert_eq!(rendered.colorspace, output.colorspace);
    let space: &Space = match output.colorspace {
        ColorSpace::Srgb => &SRGB,
        ColorSpace::DisplayP3 => &DISPLAY_P3,
    };
    let profile = icc::write(space);
    let pixels = rendered.to_u8();

    match output.format {
        OutputFormat::Jpeg => {
            let quality = output.quality.unwrap_or(92);
            let mut body = Vec::new();
            let mut encoder = jpeg_encoder::Encoder::new(&mut body, quality);
            // 4:4:4. §4 already measured what chroma subsampling costs — an iPhone
            // HEIC arrives having spent ΔE 0.9 on the RGB↔YCbCr conversion alone
            // (§3) — and there is no reason to spend it again on the way out for
            // bytes nobody is counting.
            encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
            encoder
                .encode(
                    &pixels,
                    rendered.width() as u16,
                    rendered.height() as u16,
                    jpeg_encoder::ColorType::Rgb,
                )
                .map_err(|e| ExportError::Encoder(e.to_string()))?;
            Ok(splice_jpeg_segments(&body, &profile, &metadata(source, output.metadata)))
        }
        OutputFormat::Png => {
            let mut body = Vec::new();
            {
                let mut encoder =
                    png::Encoder::new(&mut body, rendered.width(), rendered.height());
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder
                    .write_header()
                    .map_err(|e| ExportError::Encoder(e.to_string()))?;
                // The `png` crate has no ICC or EXIF helper, so both chunks are
                // written directly. They must come before the image data — PNG orders
                // `iCCP` before `IDAT`, and a reader is entitled to stop looking once
                // the pixels start.
                writer
                    .write_chunk(png::chunk::iCCP, &iccp_chunk(&profile))
                    .map_err(|e| ExportError::Encoder(e.to_string()))?;
                if let Some(bytes) = metadata(source, output.metadata) {
                    // PNG's `eXIf` holds the bare TIFF block — no `Exif\0\0` preamble,
                    // which is a JPEG marker convention rather than part of EXIF.
                    writer
                        .write_chunk(png::chunk::eXIf, &bytes)
                        .map_err(|e| ExportError::Encoder(e.to_string()))?;
                }
                writer
                    .write_image_data(&pixels)
                    .map_err(|e| ExportError::Encoder(e.to_string()))?;
            }
            Ok(body)
        }
        OutputFormat::Tiff => Err(ExportError::Unsupported {
            format: OutputFormat::Tiff,
            since: "no release has claimed it; §1's non-goals put print workflows out of scope",
        }),
    }
}

/// §6.1's `metadata` policy, as bytes to attach — or nothing.
fn metadata(source: &SourceMetadata, policy: MetadataPolicy) -> Option<Vec<u8>> {
    let exif = source.exif.as_ref()?;
    match policy {
        MetadataPolicy::Strip => None,
        MetadataPolicy::Keep => Some(exif.to_bytes(true)),
        MetadataPolicy::KeepMinusGps => Some(exif.to_bytes(false)),
    }
}

/// PNG's `iCCP`: a Latin-1 profile name, a null, the compression method, and the
/// zlib-compressed profile.
///
/// PNG has no uncompressed option for this chunk, so the profile is deflated — which
/// is why an ICC-tagged PNG is a few hundred bytes larger rather than a few thousand.
fn iccp_chunk(profile: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut out = b"PhotoDesk".to_vec();
    out.push(0);
    out.push(0); // compression method 0: zlib/deflate, the only one PNG defines
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(profile);
    out.extend_from_slice(&z.finish().unwrap_or_default());
    out
}

/// Insert APP1 (EXIF) and APP2 (ICC) segments after a JPEG's SOI.
///
/// Done here rather than through the encoder because the encoder does not offer it, and
/// because the chunking is the part that goes wrong: an ICC profile over 64 KB has to
/// be split across numbered APP2 segments, and a writer that forgets produces a file
/// whose profile silently truncates into something that still parses. The reader in
/// `decode.rs` reassembles exactly this.
fn splice_jpeg_segments(body: &[u8], profile: &[u8], exif: &Option<Vec<u8>>) -> Vec<u8> {
    const TAG: &[u8] = b"ICC_PROFILE\0";
    // A JPEG segment's length field is 16 bits and counts itself, so the payload has
    // to leave room for the length, the tag, and the two chunk bytes.
    const MAX_CHUNK: usize = 65_533 - TAG.len() - 2;

    let mut out = body[..2].to_vec(); // SOI

    if let Some(exif) = exif {
        let mut segment = exif::APP1_PREFIX.to_vec();
        segment.extend_from_slice(exif);
        if segment.len() + 2 <= 65_535 {
            out.extend_from_slice(&[0xFF, 0xE1]);
            out.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(&segment);
        }
        // An EXIF block over 64 KB cannot go in one APP1 and has no standard chunking
        // — that is what makes large previews and maker notes a separate problem. It
        // is dropped rather than truncated, because half an EXIF block is not EXIF.
    }

    let chunks: Vec<&[u8]> = profile.chunks(MAX_CHUNK).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut segment = TAG.to_vec();
        segment.push(i as u8 + 1);
        segment.push(chunks.len() as u8);
        segment.extend_from_slice(chunk);
        out.extend_from_slice(&[0xFF, 0xE2]);
        out.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&segment);
    }

    out.extend_from_slice(&body[2..]);
    out
}
