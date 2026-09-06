//! §5 stage 0: a photograph on disk becomes linear Display P3 f16 in memory.
//!
//! §4's diagram, which this module is:
//!
//! ```text
//! JPEG / HEIF   → embedded ICC → inverse EOTF → matrix → linear P3
//! Screenshot    → assume sRGB if untagged → inverse EOTF → linear P3
//! ```
//!
//! Three things about it are decisions rather than mechanics.
//!
//! **An untagged file is sRGB, and that is a guess made deliberately.** §4 says so, and
//! Spike A found the alternative in the wild: RapidRAW has no colour management at all,
//! so it reads every Display P3 photograph as sRGB — a ΔE 3.43 error that the harness
//! measures precisely because it is the mistake next door. Assuming sRGB for a file
//! that *says nothing* is the right guess; assuming it for a file that says otherwise
//! is not, and those two cases are kept apart here.
//!
//! **A missing codec is not a broken file** (§16 #13). Fedora ships libheif without
//! HEVC on patent grounds, so a stock install cannot open the format §1 calls the
//! native subject. Without a distinguishable error the symptom is "this photograph
//! will not open" and the cause is three layers away, so [`DecodeError::MissingCodec`]
//! carries the package name.
//!
//! **Orientation is applied, so `strip` can be honest.** EXIF orientation is structure
//! rather than description: a file whose pixels are sideways and whose tag says
//! "rotate me" reads correctly only to software that honours the tag, and §6.1's
//! `metadata: strip` would then rotate the photograph. Turning the pixels upright here
//! leaves the tag nothing to say. libheif already does this for HEIF; `jpeg-decoder`
//! does not, so the JPEG path does it.
//!
//! **The gain map is skipped, visibly.** §4 discards it in v1 and asks for that to be a
//! decision rather than an accident; libheif's default decode returns the SDR base, so
//! the decode reports what it stepped over rather than never looking.

use std::path::Path;

use half::f16;

use super::colour::{LINEAR_P3, SRGB, Space};
use super::icc::{self, IccError};
use super::image::Image;

/// What a decoded photograph carries besides its pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub image: Image,
    /// The space the file was interpreted as. Reported rather than assumed, because
    /// §4's whole premise is reading the manufacturer's rendering rather than guessing
    /// at it, and the user is entitled to know which happened.
    pub source_space: &'static Space,
    pub tag: ColourTag,
    /// Bits per channel as stored. §4's measurement found an iPhone HEIC's base image
    /// is 8-bit, which is what carries Spike B's f16 headroom argument onto real
    /// material.
    pub bit_depth: u8,
    /// Present and deliberately not applied (§4).
    pub gain_map: Option<GainMap>,
    /// The file carried an alpha channel, and it has been composited onto white.
    ///
    /// Reported for the same reason `gain_map` is: the pipeline has no alpha channel
    /// and v1 will not grow one, so the resolution happens at decode — and a discard
    /// the caller can see beats one it has to assume.
    pub alpha_composited: bool,
    /// The EXIF orientation found in the file, **already applied** to `image`.
    ///
    /// Reported rather than silently consumed because the document records it (§6.1's
    /// `source.orientation`) and because "1" and "6, and we turned it" are different
    /// facts about the same photograph.
    pub orientation: u8,
}

/// How the file said what space it was in — or that it did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColourTag {
    /// An embedded ICC profile, classified.
    Icc { bytes: usize },
    /// HEIF's NCLX box: coded primaries rather than a profile.
    Nclx,
    /// PNG's `sRGB` chunk: the space named rather than carried.
    ///
    /// NCLX's arrangement in a different container, and kept apart from `Untagged` for
    /// the same reason — §4's guess is defensible for a file that says nothing, and
    /// this is a file that said something.
    SrgbChunk,
    /// Nothing. §4: assume sRGB.
    Untagged,
}

/// The half-resolution auxiliary image §4 decided to discard in v1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GainMap {
    pub kind: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    /// The bytes are not a format this build reads.
    UnknownFormat { magic: [u8; 12] },
    /// §16 #13. The file is fine; this machine cannot decode it.
    MissingCodec {
        format: &'static str,
        package: &'static str,
    },
    /// The file has a profile and it is not one §4 handles.
    Profile(IccError),
    /// The decoder itself refused.
    Broken(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "{e}"),
            DecodeError::UnknownFormat { magic } => write!(
                f,
                "not a format PhotoDesk reads. The file begins {magic:02x?}"
            ),
            DecodeError::MissingCodec { format, package } => write!(
                f,
                "this file is {format}, and this machine has no {format} decoder. \
                 Fedora ships libheif without it on patent grounds; install \
                 `{package}` from RPM Fusion. The file is fine — nothing here can read \
                 it yet"
            ),
            DecodeError::Profile(e) => write!(f, "{e}"),
            DecodeError::Broken(e) => write!(f, "the decoder could not read this file: {e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        DecodeError::Io(e)
    }
}

/// The formats this build reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// HEIF/HEIC/AVIF — anything in an ISO base-media container with a `ftyp` box.
    Heif,
    Jpeg,
    Png,
}

/// PNG's signature. Eight bytes rather than four on purpose: the last four catch a
/// transfer that converted line endings or stripped the high bit, which is what they
/// were put there for (PNG 1.2 §3.1).
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Identify a file by its bytes rather than its name.
///
/// An extension is a claim by whoever last renamed the file. A `.jpg` that is really a
/// HEIC happens every time somebody's upload pipeline renames without transcoding, and
/// trusting the name turns that into "the decoder is broken".
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some(Format::Jpeg);
    }
    if bytes.len() >= 8 && bytes[..8] == PNG_MAGIC {
        return Some(Format::Png);
    }
    // An ISO base-media file starts with a four-byte length and then `ftyp`.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(Format::Heif);
    }
    None
}

/// Open a photograph and bring it into the working space.
pub fn open(path: &Path) -> Result<Decoded, DecodeError> {
    let bytes = std::fs::read(path)?;
    decode(&bytes)
}

/// As [`open`], from bytes already in hand.
pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    match sniff(bytes) {
        Some(Format::Heif) => heif::decode(bytes),
        Some(Format::Jpeg) => jpeg::decode(bytes),
        Some(Format::Png) => png::decode(bytes),
        None => {
            let mut magic = [0u8; 12];
            let n = bytes.len().min(12);
            magic[..n].copy_from_slice(&bytes[..n]);
            Err(DecodeError::UnknownFormat { magic })
        }
    }
}

/// A decoder's output buffer, described so §4's chain can be run over it once.
///
/// The three decoders hand back different shapes — libheif pads its rows, and a PNG can
/// be grey, sixteen-bit, or carry alpha — and the alternative to describing them is a
/// conversion per format. Two conversion paths would agree until they did not, which is
/// the reason §0 freezes one shader source and the reason there is one of these.
struct Surface<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
    /// Bytes per row, which is not always `width × channels × sample`: libheif pads.
    stride: usize,
    /// 1 grey, 2 grey and alpha, 3 RGB, 4 RGBA. Alpha is always last.
    channels: usize,
    depth: Depth,
}

/// Bits per sample, as the decoder handed them over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Depth {
    Eight,
    /// Big-endian pairs — how PNG stores them, and how `png` returns them.
    Sixteen,
}

impl Depth {
    fn bytes(self) -> usize {
        match self {
            Depth::Eight => 1,
            Depth::Sixteen => 2,
        }
    }

    fn levels(self) -> usize {
        match self {
            Depth::Eight => 256,
            Depth::Sixteen => 65_536,
        }
    }
}

/// A decoded surface in `space` → linear Display P3 f16, and whether an alpha channel
/// was composited away getting there.
///
/// The one place §4's chain is executed. Every decoder funnels through it so there is a
/// single answer to "what happened to this pixel", which is the same reason §0 freezes
/// one shader source: two conversion paths would agree until they did not.
fn to_working_space(surface: Surface<'_>, space: &Space) -> (Image, bool) {
    let Surface { data, width, height, stride, channels, depth } = surface;
    let matrix = space.linear_to(&LINEAR_P3);
    let mut pixels = vec![f16::ZERO; width as usize * height as usize * 3];

    // A table over every value a sample can hold, because the transfer curve is the
    // expensive part — a `powf` per channel over 12 megapixels is 36 million of them —
    // and the input is an integer. Exact rather than approximate: 256 entries for an
    // 8-bit source, 65,536 for a 16-bit one, which is 256 KB and still cheaper than the
    // first megapixel of `powf`.
    let max = (depth.levels() - 1) as f32;
    let lut: Vec<f32> = (0..depth.levels())
        .map(|code| space.transfer.to_linear(code as f32 / max))
        .collect();

    let sample = |at: usize| -> usize {
        match depth {
            Depth::Eight => data[at] as usize,
            Depth::Sixteen => u16::from_be_bytes([data[at], data[at + 1]]) as usize,
        }
    };

    let alpha = channels == 2 || channels == 4;
    let step = depth.bytes();
    for y in 0..height as usize {
        let row = y * stride;
        for x in 0..width as usize {
            let i = row + x * channels * step;
            // Grey is expanded to three channels here rather than downstream, so every
            // stage after this one sees a colour image and none of them needs a case
            // for the photograph that happens to have no chroma.
            let mut linear = match channels {
                1 | 2 => {
                    let v = lut[sample(i)];
                    [v, v, v]
                }
                _ => [
                    lut[sample(i)],
                    lut[sample(i + step)],
                    lut[sample(i + 2 * step)],
                ],
            };
            if alpha {
                // §4: the pipeline has no alpha channel, so a file that has one is
                // resolved here, onto white. In linear light, because that is the only
                // place the arithmetic is right — and the alpha sample itself never
                // goes through the curve, because opacity was never encoded by one.
                let a = sample(i + (channels - 1) * step) as f32 / max;
                for c in &mut linear {
                    *c = *c * a + (1.0 - a);
                }
            }
            let working = matrix.apply(linear);
            let o = (y * width as usize + x) * 3;
            pixels[o] = f16::from_f32(working[0]);
            pixels[o + 1] = f16::from_f32(working[1]);
            pixels[o + 2] = f16::from_f32(working[2]);
        }
    }
    (Image::new(width, height, pixels), alpha)
}

/// Classify an embedded profile, or fall back to §4's stated assumption.
fn space_from_icc(profile: &[u8]) -> Result<&'static Space, DecodeError> {
    icc::parse(profile)
        .and_then(|p| p.classify())
        .map_err(DecodeError::Profile)
}

// ------------------------------------------------------------------------- HEIF

mod heif {
    use libheif_rs::{ColorSpace, CompressionFormat, HeifContext, LibHeif, RgbChroma};

    use super::*;

    /// Apple stores the HDR gain map as an auxiliary image under this URN (§4).
    const APPLE_GAIN_MAP: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

    pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
        let lh = LibHeif::new();

        // §16 #13. Asked *before* the decode so the answer is "this machine has no
        // HEVC decoder" rather than whatever libheif says when it hits a codec it
        // cannot load — which is a generic read failure indistinguishable from a
        // truncated file.
        //
        // Checked against the container's own brand rather than assumed: an AVIF in a
        // `.heic` container needs AV1, and Fedora ships that.
        let format = brand(bytes);
        if lh.decoder_descriptors(1, Some(format)).is_empty() {
            return Err(match format {
                CompressionFormat::Hevc => DecodeError::MissingCodec {
                    format: "HEVC",
                    package: "libheif-freeworld",
                },
                CompressionFormat::Av1 => DecodeError::MissingCodec {
                    format: "AV1",
                    package: "libheif-freeworld",
                },
                _ => DecodeError::MissingCodec {
                    format: "this codec",
                    package: "libheif-freeworld",
                },
            });
        }

        let ctx = HeifContext::read_from_bytes(bytes)
            .map_err(|e| DecodeError::Broken(e.to_string()))?;
        let handle = ctx
            .primary_image_handle()
            .map_err(|e| DecodeError::Broken(e.to_string()))?;

        let (space, tag) = match (handle.color_profile_raw(), handle.color_profile_nclx()) {
            (Some(p), _) => (
                space_from_icc(&p.data)?,
                ColourTag::Icc { bytes: p.data.len() },
            ),
            // An NCLX box codes primaries by number rather than carrying a profile.
            // §4 handles two spaces and an iPhone tags P3, so this is the branch a
            // camera that codes rather than embeds lands in — not guessed silently,
            // reported as its own tag so a wrong reading is traceable.
            (None, Some(_)) => (&super::super::colour::DISPLAY_P3, ColourTag::Nclx),
            (None, None) => (&SRGB, ColourTag::Untagged),
        };

        // §4: the gain map is present and not applied. Looked for so the skip is
        // visible — "a skip we can see rather than one we assume".
        let gain_map = handle
            .auxiliary_images(None)
            .iter()
            .find_map(|aux| {
                let kind = aux.auxiliary_type().unwrap_or_default();
                (kind == APPLE_GAIN_MAP || kind.contains("gainmap")).then(|| GainMap {
                    kind,
                    width: aux.width(),
                    height: aux.height(),
                })
            });

        let bit_depth = handle.luma_bits_per_pixel();
        // The default decode: libheif returns the SDR base and does not apply the gain
        // map, which is what makes §4's v1 behaviour the thing that falls out of doing
        // nothing rather than a coincidence to rely on.
        let decoded = lh
            .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
            .map_err(|e| DecodeError::Broken(e.to_string()))?;
        let planes = decoded.planes();
        let plane = planes
            .interleaved
            .ok_or_else(|| DecodeError::Broken("no interleaved RGB plane".into()))?;

        let (image, _) = to_working_space(
            Surface {
                data: plane.data,
                width: decoded.width(),
                height: decoded.height(),
                stride: plane.stride,
                channels: 3,
                depth: Depth::Eight,
            },
            space,
        );
        // libheif applies the container's `irot`/`imir` transform properties during
        // decode, so what comes back is already upright and there is nothing to undo.
        // Recorded as 1 rather than left unstated: the document's `source.orientation`
        // describes the pixels the pipeline is holding, not the file's bookkeeping.
        Ok(Decoded {
            image,
            source_space: space,
            tag,
            bit_depth,
            gain_map,
            alpha_composited: false,
            orientation: 1,
        })
    }

    /// The container's compression format, from its `ftyp` brand.
    ///
    /// `heic`/`heix`/`hevc` are HEVC; `avif`/`avis` are AV1; `mif1` is a generic
    /// brand that iPhones use alongside `heic`, so the compatible-brand list is read
    /// too rather than only the major brand.
    fn brand(bytes: &[u8]) -> CompressionFormat {
        let ftyp_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let end = ftyp_len.clamp(16, bytes.len());
        let brands = &bytes[8..end];
        if brands.windows(4).any(|b| b == b"avif" || b == b"avis" || b == b"av01") {
            CompressionFormat::Av1
        } else {
            CompressionFormat::Hevc
        }
    }
}

// ------------------------------------------------------------------------- JPEG

mod jpeg {
    use super::*;

    pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
        let (space, tag) = match app2_icc(bytes) {
            Some(profile) => (
                space_from_icc(&profile)?,
                ColourTag::Icc { bytes: profile.len() },
            ),
            None => (&SRGB, ColourTag::Untagged),
        };

        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let pixels = decoder
            .decode()
            .map_err(|e| DecodeError::Broken(e.to_string()))?;
        let info = decoder
            .info()
            .ok_or_else(|| DecodeError::Broken("the decoder reported no image info".into()))?;

        // A greyscale JPEG is a photograph too, and `to_working_space` is where a
        // single channel becomes three — one place decides what grey means.
        let channels = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => 3,
            jpeg_decoder::PixelFormat::L8 => 1,
            other => {
                return Err(DecodeError::Broken(format!(
                    "the decoder produced {other:?}, which is not 8-bit RGB or greyscale"
                )));
            }
        };

        let (w, h) = (info.width as u32, info.height as u32);
        // `jpeg-decoder` returns the pixels as stored and says nothing about EXIF, so
        // the orientation is applied here. A file whose pixels are sideways and whose
        // tag says "rotate me" is only correct to software that reads the tag, and
        // §6.1's `metadata: strip` would then rotate the photograph.
        let orientation = crate::engine::exif::from_jpeg(bytes)
            .map(|e| e.orientation())
            .unwrap_or(1);
        let (image, _) = to_working_space(
            Surface {
                data: &pixels,
                width: w,
                height: h,
                stride: w as usize * channels,
                channels,
                depth: Depth::Eight,
            },
            space,
        );
        Ok(Decoded {
            image: image.oriented(orientation),
            source_space: space,
            tag,
            bit_depth: 8,
            gain_map: None,
            alpha_composited: false,
            orientation,
        })
    }

    /// Reassemble an ICC profile from a JPEG's APP2 `ICC_PROFILE` segments.
    ///
    /// Hand-parsed rather than pulled from the decoder, because `jpeg-decoder` does not
    /// surface it — and because the chunking is the part that goes wrong. A profile
    /// over 64 KB is split across numbered chunks that have to be concatenated **in
    /// order**; miss that and a large profile silently truncates into something that
    /// still parses, which is worse than one that does not.
    pub fn app2_icc(bytes: &[u8]) -> Option<Vec<u8>> {
        const TAG: &[u8] = b"ICC_PROFILE\0";
        let mut i = 2usize; // past SOI
        let mut chunks: Vec<(u8, &[u8])> = Vec::new();
        while i + 4 <= bytes.len() {
            if bytes[i] != 0xFF {
                break;
            }
            let marker = bytes[i + 1];
            if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
                i += 2;
                continue;
            }
            if marker == 0xDA || marker == 0xD9 {
                break; // start of scan; no more metadata
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            let segment = bytes.get(i + 4..i + 2 + len)?;
            if marker == 0xE2 && segment.len() > TAG.len() + 2 && segment.starts_with(TAG) {
                chunks.push((segment[TAG.len()], &segment[TAG.len() + 2..]));
            }
            i += 2 + len;
        }
        if chunks.is_empty() {
            return None;
        }
        chunks.sort_by_key(|(sequence, _)| *sequence);
        Some(chunks.into_iter().flat_map(|(_, d)| d.iter().copied()).collect())
    }
}

// -------------------------------------------------------------------------- PNG

/// §16 #17. The format §1's screenshot actually arrives in.
///
/// Everything colour here is the path JPEG already walks — a profile, or §4's stated
/// assumption — so what is new is the container: PNG can be grey, paletted, sixteen
/// bits deep, interlaced, or carry an alpha channel, and none of those are colour
/// decisions. `Transformations::EXPAND` and [`to_working_space`] absorb them.
mod png {
    use super::*;

    // The crate and this module share a name, so every path to the crate is written
    // from the root. Verbose, and unambiguous where a glob import made it otherwise.

    pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
        let mut decoder = ::png::Decoder::new(std::io::Cursor::new(bytes));
        // `EXPAND` turns a palette into RGB, a sub-8-bit grey into 8-bit, and a `tRNS`
        // chunk into a real alpha channel: three container features that carry no
        // colour meaning, so they are the decoder's business rather than §4's.
        //
        // It deliberately does *not* strip 16-bit samples. A 16-bit PNG is the only
        // input this build reads that carries more than eight bits per channel, and
        // spending half of it on the way into an f16 working space chosen for headroom
        // (§2.2) would be an odd thing to do.
        decoder.set_transformations(::png::Transformations::EXPAND);
        let mut reader = decoder
            .read_info()
            .map_err(|e| DecodeError::Broken(e.to_string()))?;

        let (space, tag) = {
            let info = reader.info();
            match (info.icc_profile.as_deref(), info.srgb) {
                (Some(profile), _) => (
                    space_from_icc(profile)?,
                    ColourTag::Icc { bytes: profile.len() },
                ),
                // The `sRGB` chunk names the space instead of carrying it. PNG says a
                // file should not have both, and gives `iCCP` precedence where one
                // does, which is the order matched here.
                (None, Some(_)) => (&SRGB, ColourTag::SrgbChunk),
                (None, None) => {
                    // `png` drops an `iCCP` chunk it cannot inflate and reports no
                    // profile, which arrives here indistinguishable from a file that
                    // never had one — and those two being different is §4's whole
                    // premise. So the chunk headers get walked to tell them apart.
                    if has_iccp(bytes) {
                        return Err(DecodeError::Profile(IccError::NotAProfile(
                            "an `iCCP` chunk that could not be decompressed. The file \
                             says which space it is in and the bytes saying so are \
                             damaged, which is not the same as a file that says nothing"
                                .into(),
                        )));
                    }
                    (&SRGB, ColourTag::Untagged)
                }
            }
        };

        // Recorded from the header rather than from the frame, because it describes
        // the file rather than the buffer: `EXPAND` widens a 4-bit grey to 8, and §4's
        // headroom argument is about what the photograph arrived carrying.
        let bit_depth = match reader.info().color_type {
            // On an indexed image `bit_depth` is the width of the *index* — 4 bits
            // still selects one of sixteen 8-bit colours, because `PLTE` entries are
            // always 8-bit RGB. Reporting 4 here would understate the file.
            ::png::ColorType::Indexed => 8,
            _ => reader.info().bit_depth as u8,
        };

        let size = reader
            .output_buffer_size()
            .ok_or_else(|| DecodeError::Broken("this image is too large to allocate".into()))?;
        let mut buffer = vec![0u8; size];
        // Adam7 is de-interlaced by `next_frame`, so an interlaced file needs nothing
        // here beyond not assuming rows arrive in order.
        let frame = reader
            .next_frame(&mut buffer)
            .map_err(|e| DecodeError::Broken(e.to_string()))?;

        let depth = match frame.bit_depth {
            ::png::BitDepth::Sixteen => Depth::Sixteen,
            // `EXPAND` has already widened one, two and four to eight.
            _ => Depth::Eight,
        };
        let (image, alpha_composited) = to_working_space(
            Surface {
                data: &buffer,
                width: frame.width,
                height: frame.height,
                stride: frame.line_size,
                channels: frame.color_type.samples(),
                depth,
            },
            space,
        );

        // PNG's `eXIf` holds a bare TIFF block, and PhotoDesk writes one on export — so
        // a file this app produced and reopened has an orientation to honour like any
        // other. Read after the frame rather than before it: the chunk is legal on
        // either side of `IDAT`.
        let orientation = reader
            .info()
            .exif_metadata
            .as_deref()
            .and_then(|tiff| crate::engine::exif::parse(tiff).ok())
            .map(|e| e.orientation())
            .unwrap_or(1);

        Ok(Decoded {
            image: image.oriented(orientation),
            source_space: space,
            tag,
            bit_depth,
            gain_map: None,
            alpha_composited,
            orientation,
        })
    }

    /// Whether the file contains an `iCCP` chunk, whatever shape it is in.
    ///
    /// PNG's structure makes this cheap and total: after the signature, the file is a
    /// sequence of length-tagged chunks, so the walk is arithmetic rather than parsing.
    /// It stops at `IDAT` because `iCCP` is not legal after it.
    fn has_iccp(bytes: &[u8]) -> bool {
        let mut at = PNG_MAGIC.len();
        while at + 8 <= bytes.len() {
            let len = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            let kind = &bytes[at + 4..at + 8];
            if kind == b"iCCP" {
                return true;
            }
            if kind == b"IDAT" || kind == b"IEND" {
                return false;
            }
            // Length, type, data, CRC. A length that overflows is a truncated file,
            // which the decoder will have its own opinion about.
            match at.checked_add(12).and_then(|a| a.checked_add(len as usize)) {
                Some(next) => at = next,
                None => return false,
            }
        }
        false
    }
}

/// Exposed so the colour harness can check this reassembly against a decoder that
/// does its own — the chunking is the part that goes wrong quietly.
pub use jpeg::app2_icc;
