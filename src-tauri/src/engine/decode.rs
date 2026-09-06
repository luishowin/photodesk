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
}

/// How the file said what space it was in — or that it did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColourTag {
    /// An embedded ICC profile, classified.
    Icc { bytes: usize },
    /// HEIF's NCLX box: coded primaries rather than a profile.
    Nclx,
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
}

/// Identify a file by its bytes rather than its name.
///
/// An extension is a claim by whoever last renamed the file. A `.jpg` that is really a
/// HEIC happens every time somebody's upload pipeline renames without transcoding, and
/// trusting the name turns that into "the decoder is broken".
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some(Format::Jpeg);
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
        None => {
            let mut magic = [0u8; 12];
            let n = bytes.len().min(12);
            magic[..n].copy_from_slice(&bytes[..n]);
            Err(DecodeError::UnknownFormat { magic })
        }
    }
}

/// 8-bit interleaved RGB in `space` → linear Display P3 f16.
///
/// The one place §4's chain is executed. Both decoders funnel through it so there is a
/// single answer to "what happened to this pixel", which is the same reason §0 freezes
/// one shader source: two conversion paths would agree until they did not.
fn to_working_space(
    rgb: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    space: &Space,
) -> Image {
    let matrix = space.linear_to(&LINEAR_P3);
    let mut pixels = vec![f16::ZERO; width as usize * height as usize * 3];

    // A 256-entry table, because the transfer curve is the expensive part — a `powf`
    // per channel over 12 megapixels is 36 million of them — and an 8-bit source has
    // only 256 possible inputs. Exact rather than approximate: every value the source
    // can hold is in the table.
    let mut lut = [0.0f32; 256];
    for (code, slot) in lut.iter_mut().enumerate() {
        *slot = space.transfer.to_linear(code as f32 / 255.0);
    }

    for y in 0..height as usize {
        let row = y * stride;
        for x in 0..width as usize {
            let i = row + x * 3;
            let linear = [lut[rgb[i] as usize], lut[rgb[i + 1] as usize], lut[rgb[i + 2] as usize]];
            let working = matrix.apply(linear);
            let o = (y * width as usize + x) * 3;
            pixels[o] = f16::from_f32(working[0]);
            pixels[o + 1] = f16::from_f32(working[1]);
            pixels[o + 2] = f16::from_f32(working[2]);
        }
    }
    Image::new(width, height, pixels)
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

        let image = to_working_space(
            plane.data,
            decoded.width(),
            decoded.height(),
            plane.stride,
            space,
        );
        Ok(Decoded { image, source_space: space, tag, bit_depth, gain_map })
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

        let rgb: Vec<u8> = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => pixels,
            // A greyscale JPEG is a photograph too, and expanding it here means every
            // stage downstream sees three channels and none of them needs a special
            // case.
            jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|v| [*v, *v, *v]).collect(),
            other => {
                return Err(DecodeError::Broken(format!(
                    "the decoder produced {other:?}, which is not 8-bit RGB or greyscale"
                )));
            }
        };

        let (w, h) = (info.width as u32, info.height as u32);
        let image = to_working_space(&rgb, w, h, w as usize * 3, space);
        Ok(Decoded {
            image,
            source_space: space,
            tag,
            bit_depth: 8,
            gain_map: None,
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

/// Exposed so the colour harness can check this reassembly against a decoder that
/// does its own — the chunking is the part that goes wrong quietly.
pub use jpeg::app2_icc;
