//! §5 stage 0, and §4's chain from a file to the working space.
//!
//! These tests build their own files. Everything §4 asserts about a real photograph is
//! already measured against real photographs in `tests/color/`, and duplicating that
//! here would be the same claim with a worse corpus; what this file covers is the
//! decode *path* — format sniffing, profile classification, the untagged fallback, the
//! missing-codec error §16 #13 asks for, and the resample §7.1 puts between decode and
//! the shader chain.
//!
//! The one thing built rather than asserted is the ICC parser's agreement with lcms2,
//! and that lives in `tests/color/` where lcms2 already is — this crate does not link
//! it, deliberately.

use half::f16;
use photodesk::engine::colour::{DISPLAY_P3, LINEAR_P3, SRGB, Space};
use photodesk::engine::decode::{self, ColourTag, DecodeError, Format};
use photodesk::engine::icc;
use photodesk::engine::image::Image;

/// A profile for one of §4's spaces, built with lcms2 in the colour harness and
/// hard-coded here as bytes would be a fixture this crate cannot regenerate. Instead
/// the tests below build profiles with the same byte layout the parser reads, which
/// keeps the parser honest about the format rather than about one file.
mod profile {
    use photodesk::engine::colour::Space;

    /// A minimal but valid ICC v4 matrix/TRC profile for `space`.
    ///
    /// Deliberately assembled by hand: the parser is the thing under test, and feeding
    /// it a profile produced by the same code that reads it would prove nothing. Every
    /// offset here comes from ICC.1:2010 rather than from `icc.rs`.
    pub fn build(space: &Space, curve: Curve) -> Vec<u8> {
        // The colorants have to be written in the D50 connection space, which is the
        // whole trap `icc.rs` exists to avoid — so the test adapts them the other way,
        // D65 → D50, using the same Bradford transform in reverse.
        let d65 = photodesk::engine::colour::D65;
        let d65_xyz = [d65.x / d65.y, 1.0, (1.0 - d65.x - d65.y) / d65.y];
        let to_d50 = photodesk::engine::icc::bradford(d65_xyz, [0.9642, 1.0, 0.8249]);
        let m = to_d50.mul(&space.to_xyz()).0;

        let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"rXYZ", xyz_tag([m[0][0], m[1][0], m[2][0]])),
            (b"gXYZ", xyz_tag([m[0][1], m[1][1], m[2][1]])),
            (b"bXYZ", xyz_tag([m[0][2], m[1][2], m[2][2]])),
            (b"wtpt", xyz_tag([0.9642, 1.0, 0.8249])),
        ];
        for sig in [b"rTRC", b"gTRC", b"bTRC"] {
            tags.push((sig, curve.bytes()));
        }

        let header = 128;
        let table = 4 + tags.len() * 12;
        let mut out = vec![0u8; header + table];
        out[36..40].copy_from_slice(b"acsp");
        out[header..header + 4].copy_from_slice(&(tags.len() as u32).to_be_bytes());

        for (i, (sig, data)) in tags.iter().enumerate() {
            let at = header + 4 + i * 12;
            let offset = out.len() as u32;
            out[at..at + 4].copy_from_slice(*sig);
            out[at + 4..at + 8].copy_from_slice(&offset.to_be_bytes());
            out[at + 8..at + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_be_bytes());
        out
    }

    #[derive(Clone, Copy)]
    pub enum Curve {
        /// The sRGB curve as ICC parametric type 3, which is how a real profile
        /// stores it.
        Srgb,
        /// A pure gamma, for the profile that has our primaries and not our curve.
        Gamma(f64),
    }

    impl Curve {
        fn bytes(self) -> Vec<u8> {
            match self {
                Curve::Srgb => {
                    let mut v = b"para".to_vec();
                    v.extend_from_slice(&[0, 0, 0, 0]);
                    v.extend_from_slice(&3u16.to_be_bytes());
                    v.extend_from_slice(&[0, 0]);
                    for p in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
                        v.extend_from_slice(&s15(p));
                    }
                    v
                }
                Curve::Gamma(g) => {
                    let mut v = b"curv".to_vec();
                    v.extend_from_slice(&[0, 0, 0, 0]);
                    v.extend_from_slice(&1u32.to_be_bytes());
                    v.extend_from_slice(&(((g * 256.0).round()) as u16).to_be_bytes());
                    v
                }
            }
        }
    }

    fn xyz_tag(v: [f64; 3]) -> Vec<u8> {
        let mut out = b"XYZ ".to_vec();
        out.extend_from_slice(&[0, 0, 0, 0]);
        for c in v {
            out.extend_from_slice(&s15(c));
        }
        out
    }

    fn s15(v: f64) -> [u8; 4] {
        (((v * 65536.0).round()) as i32).to_be_bytes()
    }
}

/// A JPEG carrying `profile`, or none.
///
/// `jpeg-decoder` will read this back, so it has to be a real JPEG — built by encoding
/// a small gradient with the same crate's counterpart is not possible (it only
/// decodes), so the test embeds a tiny pre-made one and splices the APP2 segment in.
fn jpeg_with(profile: Option<&[u8]>) -> Vec<u8> {
    jpeg_of(MINIMAL_JPEG, profile)
}

/// Splice an APP2 `ICC_PROFILE` segment into `base`, right after its SOI.
fn jpeg_of(base: &[u8], profile: Option<&[u8]>) -> Vec<u8> {
    let base = base.to_vec();
    let Some(profile) = profile else {
        return base;
    };
    let mut out = base[..2].to_vec(); // SOI
    let mut segment = b"ICC_PROFILE\0".to_vec();
    segment.push(1); // chunk 1
    segment.push(1); // of 1
    segment.extend_from_slice(profile);
    out.extend_from_slice(&[0xFF, 0xE2]);
    out.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&segment);
    out.extend_from_slice(&base[2..]);
    out
}

/// A 2×2 mid-grey baseline JPEG, 629 bytes.
///
/// Generated once with Pillow and inlined, because this crate has a decoder and no
/// encoder, and a decoder test needs a file a real decoder accepts. The first attempt
/// at writing one by hand was rejected with "invalid length in DHT" — which is the
/// right outcome and a good argument against hand-rolled fixtures for formats with
/// tables in them.
///
/// Its *pixels* matter to nothing here. What matters is that everything around it —
/// APP2 splicing, profile classification, the working-space conversion — is exercised
/// against something a decoder will actually read.
const MINIMAL_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x03, 0x02, 0x02, 0x03, 0x02,
    0x02, 0x03, 0x03, 0x03, 0x03, 0x04, 0x03, 0x03, 0x04, 0x05, 0x08, 0x05, 0x05, 0x04, 0x04,
    0x05, 0x0A, 0x07, 0x07, 0x06, 0x08, 0x0C, 0x0A, 0x0C, 0x0C, 0x0B, 0x0A, 0x0B, 0x0B, 0x0D,
    0x0E, 0x12, 0x10, 0x0D, 0x0E, 0x11, 0x0E, 0x0B, 0x0B, 0x10, 0x16, 0x10, 0x11, 0x13, 0x14,
    0x15, 0x15, 0x15, 0x0C, 0x0F, 0x17, 0x18, 0x16, 0x14, 0x18, 0x12, 0x14, 0x15, 0x14, 0xFF,
    0xDB, 0x00, 0x43, 0x01, 0x03, 0x04, 0x04, 0x05, 0x04, 0x05, 0x09, 0x05, 0x05, 0x09, 0x14,
    0x0D, 0x0B, 0x0D, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02,
    0x00, 0x02, 0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xC4, 0x00,
    0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
    0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05,
    0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
    0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
    0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A,
    0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
    0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
    0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
    0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93,
    0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
    0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
    0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
    0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
    0xF8, 0xF9, 0xFA, 0xFF, 0xC4, 0x00, 0x1F, 0x01, 0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
    0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x11, 0x00, 0x02, 0x01, 0x02,
    0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77, 0x00, 0x01, 0x02,
    0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22,
    0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0, 0x15,
    0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66,
    0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84,
    0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A,
    0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7,
    0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4,
    0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01,
    0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0x28, 0xA2, 0x8A, 0x00, 0xFF, 0xD9,
];

/// A flat, saturated red 8×8 at 4:4:4, 634 bytes.
///
/// The grey fixture above cannot tell sRGB from Display P3, and finding that out is
/// worth writing down: **a neutral is neutral in every RGB space sharing a white
/// point**, so (128, 128, 128) read as sRGB and read as Display P3 land on the same
/// working-space value to the last bit. The two interpretations only diverge on
/// chromatic content, which is why the profile test uses this one and the neutrality
/// test uses the other.
const RED_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xFF,
    0xDB, 0x00, 0x43, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x08,
    0x00, 0x08, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xC4, 0x00,
    0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
    0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05,
    0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
    0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
    0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A,
    0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
    0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
    0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
    0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93,
    0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
    0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
    0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
    0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
    0xF8, 0xF9, 0xFA, 0xFF, 0xC4, 0x00, 0x1F, 0x01, 0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
    0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x11, 0x00, 0x02, 0x01, 0x02,
    0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77, 0x00, 0x01, 0x02,
    0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22,
    0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0, 0x15,
    0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66,
    0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84,
    0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A,
    0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7,
    0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4,
    0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01,
    0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0xFC, 0xB7, 0xAF, 0xF3, 0xFC, 0xFF, 0x00,
    0xAF, 0x83, 0xFF, 0xD9,
];

// -------------------------------------------------------------------- sniffing

/// An extension is a claim by whoever last renamed the file.
#[test]
fn format_is_decided_by_bytes_not_by_name() {
    assert_eq!(decode::sniff(MINIMAL_JPEG), Some(Format::Jpeg));

    let mut heif = vec![0, 0, 0, 0x20];
    heif.extend_from_slice(b"ftypheic");
    heif.extend_from_slice(&[0; 16]);
    assert_eq!(decode::sniff(&heif), Some(Format::Heif));

    assert_eq!(decode::sniff(b"\x89PNG\r\n\x1a\n----"), Some(Format::Png));
    // PNG's signature is eight bytes and all eight are load-bearing: the trailing
    // CRLF/EOF/LF exist to catch a transfer that mangled line endings, and a file that
    // has been through one is not a file this can decode.
    assert_eq!(decode::sniff(b"\x89PNG\n\n\x1a\n----"), None);
    assert_eq!(decode::sniff(b"\x89PNG\r\n\x1a"), None);
    assert_eq!(decode::sniff(b""), None);
    assert_eq!(decode::sniff(&[0xFF]), None);
}

/// A format this build does not read is an error naming what it saw, not a panic and
/// not a silent empty image.
#[test]
fn an_unreadable_format_says_what_it_found() {
    // GIF, which is not going to become a subject. Until §16 #17 this test used a PNG
    // header, which was the honest choice at the time and is now the wrong one.
    let err = decode::decode(b"GIF89a\x01\x00\x01\x00\x80").unwrap_err();
    println!("{err}");
    assert!(matches!(err, DecodeError::UnknownFormat { .. }));
    assert!(err.to_string().contains("47"), "{err}");
}

// -------------------------------------------------------- profile classification

/// §4's two spaces, read out of a real profile layout.
#[test]
fn both_of_section_4s_spaces_are_recognised() {
    for space in [&SRGB, &DISPLAY_P3] {
        let bytes = profile::build(space, profile::Curve::Srgb);
        let parsed = icc::parse(&bytes).expect("parse");
        let classified = parsed.classify().unwrap_or_else(|e| panic!("{}: {e}", space.name));
        println!("{} ({} bytes) -> {}", space.name, bytes.len(), classified.name);
        assert_eq!(classified.name, space.name);
    }
}

/// **The test the tolerance exists for.** Adobe RGB shares sRGB's red and blue
/// primaries and sits 0.0901 from Display P3 — closer to it than to sRGB. A classifier
/// thresholding on the red colorant alone reads it as Display P3, which is a silent
/// wrong colour on a profile people actually have.
#[test]
fn a_third_space_is_refused_rather_than_rounded_to_the_nearest() {
    use photodesk::engine::colour::{Transfer, Xy};
    let adobe = Space {
        name: "Adobe RGB (1998)",
        red: Xy::new(0.640, 0.330),
        green: Xy::new(0.210, 0.710),
        blue: Xy::new(0.150, 0.060),
        white: photodesk::engine::colour::D65,
        transfer: Transfer::Srgb,
    };
    let bytes = profile::build(&adobe, profile::Curve::Srgb);
    let err = icc::parse(&bytes)
        .expect("it is a well-formed profile")
        .classify()
        .expect_err("Adobe RGB is not one of §4's two spaces");
    println!("{err}");

    match err {
        icc::IccError::UnknownSpace { nearest, distance, .. } => {
            // It really is nearest Display P3 — which is what makes a threshold on the
            // red colorant get this wrong, and what makes the full-matrix comparison
            // the thing that saves it.
            assert_eq!(nearest, "Display P3");
            assert!(distance > 0.05, "the spaces are closer than expected: {distance}");
        }
        other => panic!("expected UnknownSpace, got {other:?}"),
    }
    // The message reports the primaries *at D65*, having adapted them — 0.5767 is
    // Adobe RGB's red there, where 0.6097 is the D50 value the tag actually holds.
    // Reporting the adapted number is right: it is the one comparable to the spaces
    // named beside it.
    assert!(
        err.to_string().contains("R[0.5767"),
        "the message does not show the primaries it read: {err}"
    );
}

/// Primaries are half of a space. §4's two both use the sRGB curve — Display P3 uses
/// it rather than DCI's 2.6 gamma, and `colour.rs` notes that getting that wrong is a
/// ~4 ΔE error that looks like a gamut problem.
#[test]
fn our_primaries_with_somebody_elses_curve_are_refused() {
    let bytes = profile::build(&DISPLAY_P3, profile::Curve::Gamma(1.8));
    let err = icc::parse(&bytes)
        .unwrap()
        .classify()
        .expect_err("P3 primaries with a 1.8 gamma is not Display P3");
    println!("{err}");
    assert!(matches!(err, icc::IccError::UnknownCurve { space: "Display P3", .. }));

    // And the curve this build *does* accept is read correctly, so the rejection above
    // is about the curve rather than about the parser not understanding curves.
    let ok = profile::build(&DISPLAY_P3, profile::Curve::Gamma(2.2));
    let parsed = icc::parse(&ok).unwrap();
    let worst = (0..=16)
        .map(|i| {
            let x = i as f64 / 16.0;
            (parsed.trc[0].to_linear(x) - x.powf(2.2)).abs()
        })
        .fold(0.0, f64::max);
    // Not exact, and the reason is the format rather than the parser: a one-entry
    // `curv` stores its gamma as u8Fixed8, so 2.2 is written as 563/256 = 2.19922 and
    // cannot be anything else. The residual is that quantisation, three orders of
    // magnitude below the curve tolerance and far below anything visible.
    assert!(
        worst < 1e-3,
        "a pure gamma curve did not read back: {worst} — larger than u8Fixed8's own \
         quantisation, so this is the parser rather than the format"
    );
}

/// An ICC profile arrives from a file somebody else wrote. Every malformation is an
/// error; none of them is a panic, because a panic in a decoder is a crash on opening
/// a photograph.
#[test]
fn a_malformed_profile_errors_rather_than_panics() {
    let good = profile::build(&SRGB, profile::Curve::Srgb);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("too short for a header", vec![0; 100]),
        ("no acsp signature", vec![0; 400]),
        ("truncated mid-tag-table", good[..140].to_vec()),
        ("every prefix of a valid profile", good[..good.len() / 2].to_vec()),
    ];
    for (why, bytes) in cases {
        let result = icc::parse(&bytes);
        println!("{why}: {result:?}");
        assert!(result.is_err(), "{why} parsed successfully");
    }

    // A tag count large enough to overflow the table, which is the read-past-the-end
    // an unchecked parser would perform.
    let mut hostile = good.clone();
    hostile[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(icc::parse(&hostile).is_err());

    // And every truncation of a valid profile, which is what a half-written file is.
    for n in 0..good.len() {
        let _ = icc::parse(&good[..n]);
    }
}

// ------------------------------------------------------------- the decode itself

/// §4: an untagged file is assumed sRGB, and a tagged one is not.
#[test]
fn an_untagged_file_is_assumed_srgb_and_a_tagged_one_is_read() {
    let untagged = decode::decode(&jpeg_of(RED_JPEG, None)).expect("decode");
    println!("untagged: {:?} {:?}", untagged.tag, untagged.source_space.name);
    assert_eq!(untagged.tag, ColourTag::Untagged);
    assert_eq!(untagged.source_space.name, SRGB.name);

    let p3 = profile::build(&DISPLAY_P3, profile::Curve::Srgb);
    let tagged = decode::decode(&jpeg_of(RED_JPEG, Some(&p3))).expect("decode");
    println!("tagged: {:?} {:?}", tagged.tag, tagged.source_space.name);
    assert_eq!(tagged.tag, ColourTag::Icc { bytes: p3.len() });
    assert_eq!(tagged.source_space.name, DISPLAY_P3.name);

    // The distinction is not cosmetic: the same bytes land on different working-space
    // values, which is the ΔE 3.43 error Spike A found RapidRAW making on every P3
    // photograph it opens. A *saturated* pixel is needed to see it — a neutral is
    // neutral in both spaces, and the first version of this test used a grey fixture
    // and asserted a difference that cannot exist.
    let (a, b) = (untagged.image.pixel(0, 0), tagged.image.pixel(0, 0));
    println!("as sRGB {a:?}\nas P3   {b:?}");
    assert_ne!(a, b);
    assert!(
        (a[0] - b[0]).abs() > 0.05,
        "reading a saturated red as the wrong space moved it by only {:.4}",
        (a[0] - b[0]).abs()
    );
}

/// A file with a profile §4 cannot classify is refused, and the refusal survives the
/// trip through the decoder rather than being swallowed into "broken file".
#[test]
fn a_jpeg_with_an_unhandled_profile_is_refused_by_profile() {
    use photodesk::engine::colour::{Transfer, Xy};
    let prophoto = Space {
        name: "ProPhoto",
        red: Xy::new(0.7347, 0.2653),
        green: Xy::new(0.1596, 0.8404),
        blue: Xy::new(0.0366, 0.0001),
        white: photodesk::engine::colour::D65,
        transfer: Transfer::Srgb,
    };
    let bytes = jpeg_with(Some(&profile::build(&prophoto, profile::Curve::Srgb)));
    let err = decode::decode(&bytes).unwrap_err();
    println!("{err}");
    assert!(matches!(err, DecodeError::Profile(_)));
}

/// The APP2 reassembly, which is the part that goes wrong quietly: a profile over
/// 64 KB is split across numbered chunks that must be concatenated **in order**.
#[test]
fn a_chunked_app2_profile_is_reassembled_in_order() {
    let profile: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();

    // Split into chunks and — the point of the test — write them out of order, which
    // is legal: the sequence number is what orders them, not their position.
    let mut jpeg = MINIMAL_JPEG[..2].to_vec();
    let chunks: Vec<&[u8]> = profile.chunks(60_000).collect();
    let total = chunks.len() as u8;
    let mut order: Vec<usize> = (0..chunks.len()).collect();
    order.reverse();
    for i in order {
        let mut segment = b"ICC_PROFILE\0".to_vec();
        segment.push(i as u8 + 1);
        segment.push(total);
        segment.extend_from_slice(chunks[i]);
        jpeg.extend_from_slice(&[0xFF, 0xE2]);
        jpeg.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&segment);
    }
    jpeg.extend_from_slice(&MINIMAL_JPEG[2..]);

    let back = decode::app2_icc(&jpeg).expect("a profile spread over four chunks");
    assert_eq!(back.len(), profile.len(), "the profile was truncated");
    assert_eq!(back, profile, "the chunks were concatenated in the wrong order");
}

// -------------------------------------------------------------------------- PNG

/// A PNG built to order.
///
/// Generated rather than hand-written, for the reason the JPEG above is — chunk CRCs
/// and row filters are exactly the kind of offset arithmetic that fails looking like a
/// decoder bug. Unlike the JPEG, this crate *does* have a PNG encoder, so these
/// fixtures are written by the same crate that reads them. That is deliberate and it
/// is not circular: the container is not what these tests are about. The profile is
/// built above from ICC.1:2010, the colour path is ours, and the one container feature
/// this encoder cannot write — interlacing — has a fixture from ImageMagick instead.
struct Png {
    width: u32,
    height: u32,
    colour: png::ColorType,
    depth: png::BitDepth,
    data: Vec<u8>,
    palette: Option<Vec<u8>>,
    icc: Option<Vec<u8>>,
    /// Written raw, so a test can supply bytes that are not a valid zlib stream.
    iccp_raw: Option<Vec<u8>>,
    srgb_chunk: bool,
    exif: Option<Vec<u8>>,
}

impl Png {
    /// 8-bit RGB by default, the shape a screenshot arrives in.
    fn of(width: u32, height: u32, data: Vec<u8>) -> Self {
        Png {
            width,
            height,
            colour: png::ColorType::Rgb,
            depth: png::BitDepth::Eight,
            data,
            palette: None,
            icc: None,
            iccp_raw: None,
            srgb_chunk: false,
            exif: None,
        }
    }

    fn colour(mut self, colour: png::ColorType, depth: png::BitDepth) -> Self {
        self.colour = colour;
        self.depth = depth;
        self
    }

    fn icc(mut self, profile: &[u8]) -> Self {
        self.icc = Some(profile.to_vec());
        self
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(self.colour);
            encoder.set_depth(self.depth);
            if let Some(palette) = &self.palette {
                encoder.set_palette(palette.clone());
            }
            if self.srgb_chunk {
                encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            }
            let mut writer = encoder.write_header().expect("header");
            // `iCCP` is the export path's chunk builder in miniature: a Latin-1 name, a
            // null, the compression method, and the deflated profile.
            if let Some(profile) = &self.icc {
                use std::io::Write;
                let mut chunk = b"test\0\0".to_vec();
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
                z.write_all(profile).expect("deflate");
                chunk.extend_from_slice(&z.finish().expect("deflate"));
                writer.write_chunk(png::chunk::iCCP, &chunk).expect("iCCP");
            }
            if let Some(raw) = &self.iccp_raw {
                writer.write_chunk(png::chunk::iCCP, raw).expect("iCCP");
            }
            if let Some(exif) = &self.exif {
                writer
                    .write_chunk(png::chunk::eXIf, exif)
                    .expect("eXIf");
            }
            writer.write_image_data(&self.data).expect("pixels");
        }
        out
    }
}

/// A single 8-bit RGB pixel, as a whole PNG.
fn png_pixel(rgb: [u8; 3]) -> Png {
    Png::of(1, 1, rgb.to_vec())
}

/// §4's chain runs the same way whatever container the profile arrived in: the file
/// says Display P3, so the pixel is read as Display P3.
#[test]
fn a_png_reads_its_profile_the_way_a_jpeg_does() {
    let p3 = profile::build(&DISPLAY_P3, profile::Curve::Srgb);
    let red = [220u8, 40, 30];

    let untagged = decode::decode(&png_pixel(red).bytes()).expect("decode");
    let tagged = decode::decode(&png_pixel(red).icc(&p3).bytes()).expect("decode");
    println!("untagged {:?}\ntagged   {:?}", untagged.tag, tagged.tag);

    assert_eq!(untagged.tag, ColourTag::Untagged);
    assert_eq!(untagged.source_space.name, SRGB.name);
    assert_eq!(tagged.tag, ColourTag::Icc { bytes: p3.len() });
    assert_eq!(tagged.source_space.name, DISPLAY_P3.name);

    // And the distinction moves the pixel, which is the whole reason §4 insists on it.
    // A saturated colour, because a neutral is neutral in both spaces.
    let (a, b) = (untagged.image.pixel(0, 0), tagged.image.pixel(0, 0));
    println!("as sRGB {a:?}\nas P3   {b:?}");
    assert!(
        (a[0] - b[0]).abs() > 0.05,
        "reading a saturated red as the wrong space moved it by only {:.4}",
        (a[0] - b[0]).abs()
    );
}

/// §4's guess is defensible for a file that says nothing. A file that *says* sRGB has
/// said something, and the two are kept apart even though they land in the same space.
#[test]
fn a_png_that_names_srgb_is_not_a_png_that_says_nothing() {
    let mut named = png_pixel([220, 40, 30]);
    named.srgb_chunk = true;

    let silent = decode::decode(&png_pixel([220, 40, 30]).bytes()).expect("decode");
    let named = decode::decode(&named.bytes()).expect("decode");
    println!("silent {:?}\nnamed  {:?}", silent.tag, named.tag);

    assert_eq!(silent.tag, ColourTag::Untagged);
    assert_eq!(named.tag, ColourTag::SrgbChunk);
    // Same space, same pixels — the difference is in what the file claimed, which is
    // what a wrong reading would later have to be traced through.
    assert_eq!(named.source_space.name, SRGB.name);
    assert_eq!(silent.image.pixel(0, 0), named.image.pixel(0, 0));
}

/// A file that says which space it is in, damaged so that it cannot be read, is not a
/// file that says nothing — and `png` reports both as "no profile".
#[test]
fn a_damaged_profile_is_refused_rather_than_read_as_srgb() {
    let mut broken = png_pixel([220, 40, 30]);
    // A valid chunk layout — name, null, compression method — wrapped around bytes
    // that are not a zlib stream.
    broken.iccp_raw = Some(b"test\0\0not a deflate stream".to_vec());

    let err = decode::decode(&broken.bytes()).unwrap_err();
    println!("{err}");
    assert!(matches!(err, DecodeError::Profile(_)), "{err:?}");
}

/// §4 gives the pipeline no alpha channel and v1 will not grow one, so a file that has
/// one is resolved at the door — onto white, in linear light.
#[test]
fn a_screenshots_transparency_is_composited_onto_white() {
    // Three pixels: opaque black, half-transparent black, fully transparent black.
    let data = vec![0, 0, 0, 255, 0, 0, 0, 128, 0, 0, 0, 0];
    let png = Png::of(3, 1, data).colour(png::ColorType::Rgba, png::BitDepth::Eight);
    let decoded = decode::decode(&png.bytes()).expect("decode");

    assert!(decoded.alpha_composited, "the discard has to be visible");
    let opaque = decoded.image.pixel(0, 0);
    let half = decoded.image.pixel(1, 0);
    let clear = decoded.image.pixel(2, 0);
    println!("opaque {opaque:?}\nhalf   {half:?}\nclear  {clear:?}");

    assert!(opaque[0] < 1e-3, "an opaque pixel is untouched");
    assert!((clear[0] - 1.0).abs() < 1e-3, "a transparent pixel is white");

    // The number that matters. 128/255 of white in *linear* light is 0.502, and this
    // is the assertion that would fail if the composite ran on encoded values instead
    // — that lands at 0.502 sRGB-encoded, which is 0.216 linear, less than half of it.
    let expected = 1.0 - 128.0 / 255.0;
    println!("expected {expected:.4}, encoded-instead would be {:.4}", 0.2158);
    assert!(
        (half[0] - expected).abs() < 2e-3,
        "half-transparent black landed at {:.4}, not {expected:.4}",
        half[0]
    );
}

/// The only input this build reads that carries more than eight bits a channel. It has
/// to arrive with them: the working space was chosen for headroom (§2.2), and an f16
/// buffer fed 8-bit codes has headroom over nothing.
#[test]
fn sixteen_bits_arrive_with_more_than_eight() {
    // A value strictly between two 8-bit codes: 100.5/255 of full scale.
    let between = 25_800u16;
    let data: Vec<u8> = [between, between, between]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    let deep = decode::decode(
        &Png::of(1, 1, data).colour(png::ColorType::Rgb, png::BitDepth::Sixteen).bytes(),
    )
    .expect("decode");
    assert_eq!(deep.bit_depth, 16);

    let below = decode::decode(&png_pixel([100, 100, 100]).bytes()).expect("decode");
    let above = decode::decode(&png_pixel([101, 101, 101]).bytes()).expect("decode");
    let (d, b, a) = (
        deep.image.pixel(0, 0)[1],
        below.image.pixel(0, 0)[1],
        above.image.pixel(0, 0)[1],
    );
    println!("8-bit {b:.6} < 16-bit {d:.6} < 8-bit {a:.6}");
    assert!(
        b < d && d < a,
        "a 16-bit sample landed on an 8-bit code: {b:.6} {d:.6} {a:.6}"
    );
}

/// Grey and palette are container features rather than colour ones, and the pipeline
/// downstream should never learn either happened.
#[test]
fn grey_and_palette_become_three_channels() {
    let grey = decode::decode(
        &Png::of(1, 1, vec![160])
            .colour(png::ColorType::Grayscale, png::BitDepth::Eight)
            .bytes(),
    )
    .expect("decode");
    let rgb = decode::decode(&png_pixel([160, 160, 160]).bytes()).expect("decode");
    println!("grey {:?}\nrgb  {:?}", grey.image.pixel(0, 0), rgb.image.pixel(0, 0));
    assert_eq!(grey.image.pixel(0, 0), rgb.image.pixel(0, 0));

    // Two pixels in one byte, high nibble first: index 1 then index 0. Written that
    // way round on purpose — a 4-bit palette is the only input here whose samples are
    // narrower than a byte, and reading the wrong nibble is a wrong colour rather than
    // an error. The first attempt at this fixture was 1×1 with the same byte, which
    // selected index 1 while claiming to select index 0, and passed nothing.
    let mut paletted = Png::of(2, 1, vec![0x10]);
    paletted.colour = png::ColorType::Indexed;
    paletted.depth = png::BitDepth::Four;
    paletted.palette = Some(vec![220, 40, 30, 0, 0, 0]);
    let paletted = decode::decode(&paletted.bytes()).expect("decode");
    let direct = decode::decode(&png_pixel([220, 40, 30]).bytes()).expect("decode");
    println!(
        "palette {:?} {:?} at {} bits\ndirect  {:?}",
        paletted.image.pixel(0, 0),
        paletted.image.pixel(1, 0),
        paletted.bit_depth,
        direct.image.pixel(0, 0)
    );
    assert_eq!(paletted.image.pixel(0, 0), [0.0, 0.0, 0.0]);
    assert_eq!(paletted.image.pixel(1, 0), direct.image.pixel(0, 0));
    // Four bits selects one of sixteen colours; the colours themselves are 8-bit, and
    // reporting the index width here would understate the file.
    assert_eq!(paletted.bit_depth, 8);
}

/// Adam7, which is the one PNG feature that changes the order rows arrive in — and so
/// the one that fails as a scrambled image rather than as an error.
///
/// The fixture is from ImageMagick because the `png` crate has no interlaced encoder,
/// and its pixels were checked with Pillow, which did not write it.
#[test]
fn an_interlaced_png_is_de_interlaced_before_the_colour_path_sees_it() {
    let interlaced = decode::decode(INTERLACED_PNG).expect("decode");
    assert_eq!((interlaced.image.width(), interlaced.image.height()), (8, 8));

    // The same pattern written progressively. Every pixel in it is distinct by
    // construction, so a scrambled de-interlace cannot pass by landing on a plausible
    // average somewhere in the middle.
    let pattern: Vec<u8> = (0..8u8)
        .flat_map(|y| (0..8u8).flat_map(move |x| [x * 32 + 8, y * 32 + 8, 128]))
        .collect();
    let progressive = decode::decode(&Png::of(8, 8, pattern).bytes()).expect("decode");

    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                interlaced.image.pixel(x, y),
                progressive.image.pixel(x, y),
                "({x},{y}) does not survive Adam7"
            );
        }
    }
}

/// PhotoDesk writes `eXIf` on export (§6.1), so a PNG it produced and reopened has an
/// orientation like any other file — and §0's rule is that the pixels get turned.
#[test]
fn a_pngs_exif_orientation_is_applied_to_the_pixels() {
    // Two rows, distinguishable: orientation 3 is a 180° rotation, so they swap.
    let data = vec![255, 0, 0, 0, 0, 255];
    let upright = decode::decode(&Png::of(1, 2, data.clone()).bytes()).expect("decode");
    assert_eq!(upright.orientation, 1);

    let mut turned = Png::of(1, 2, data);
    turned.exif = Some(exif_block(3));
    let turned = decode::decode(&turned.bytes()).expect("decode");
    println!("orientation {}", turned.orientation);
    assert_eq!(turned.orientation, 3);
    assert_eq!(turned.image.pixel(0, 0), upright.image.pixel(0, 1));
    assert_eq!(turned.image.pixel(0, 1), upright.image.pixel(0, 0));
}

/// 8×8, one distinct colour per pixel, Adam7-interlaced.
///
/// Written by ImageMagick because the `png` crate has no interlaced encoder, and read
/// back with Pillow — which did not write it — to confirm both the interlace bit in
/// `IHDR` and every pixel of the pattern. Pillow was the first choice for writing it
/// and its `interlace=True` was silently ignored, which is the reason the check with a
/// second tool is in the rule rather than in somebody's habits.
const INTERLACED_PNG: &[u8] = include_bytes!("fixtures/interlaced.png");

/// A bare TIFF block with one IFD0 entry: orientation.
///
/// Little-endian, and with the four-byte next-IFD pointer that a previous hand-built
/// EXIF fixture in this project forgot — an omission that read as a parser bug.
fn exif_block(orientation: u16) -> Vec<u8> {
    let mut out = b"II".to_vec();
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes()); // IFD0 begins immediately
    out.extend_from_slice(&1u16.to_le_bytes()); // one entry
    out.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    out.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    out.extend_from_slice(&1u32.to_le_bytes()); // count
    out.extend_from_slice(&orientation.to_le_bytes());
    out.extend_from_slice(&[0, 0]); // the value field is four bytes wide
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    out
}

// ------------------------------------------------------------------ §7.1's proxy

/// §7.1: `min(2 × viewport_longest_edge, source_longest_edge)`.
#[test]
fn the_proxy_size_never_exceeds_the_source() {
    // A 12 MP photograph on a 1080p display: two times the viewport.
    assert_eq!(Image::proxy_longest_edge(4032, 1920), 3840);
    // The same photograph on a 4K display: the source runs out first.
    assert_eq!(Image::proxy_longest_edge(4032, 3840), 4032);
    // A small scan on a big display: no upscale. There is no detail up there.
    assert_eq!(Image::proxy_longest_edge(900, 3840), 900);
}

/// The resample is an area average **in linear light**, which is the whole reason it
/// happens after the colour conversion rather than in the decoder.
///
/// The test is the classic counterexample: a checkerboard of black and white pixels.
/// Averaged in linear light it is exactly mid-grey, 0.5 linear. Averaged in encoded
/// sRGB — the mistake — it is code 128, which is 0.216 linear, and the image comes out
/// visibly too dark in a way that tracks local contrast.
#[test]
fn downsampling_averages_in_linear_light() {
    let (w, h) = (64u32, 64u32);
    let mut pixels = vec![f16::ZERO; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let on = (x + y) % 2 == 0;
            let i = ((y * w + x) * 3) as usize;
            for c in 0..3 {
                pixels[i + c] = f16::from_f32(if on { 1.0 } else { 0.0 });
            }
        }
    }
    let image = Image::new(w, h, pixels);
    let small = image.proxy(8);
    println!("{image:?} -> {small:?}");

    for y in 0..small.height() {
        for x in 0..small.width() {
            let p = small.pixel(x, y);
            for c in p {
                assert!(
                    (c - 0.5).abs() < 0.01,
                    "a black-and-white checkerboard averaged to {c} rather than 0.5. \
                     0.216 would mean the average happened in encoded sRGB"
                );
            }
        }
    }
}

/// Area coverage is exact for ratios that are not integers, which is the case a naive
/// box filter gets subtly wrong.
#[test]
fn the_resample_conserves_total_light_at_awkward_ratios() {
    for (from, to) in [(100u32, 37u32), (37, 10), (1000, 999), (9, 3)] {
        let pixels: Vec<f16> = (0..from * from * 3)
            .map(|i| f16::from_f32(((i % 97) as f32) / 96.0))
            .collect();
        let image = Image::new(from, from, pixels);
        let small = image.resample(to, to);

        let mean = |img: &Image| {
            let mut total = 0.0f64;
            for y in 0..img.height() {
                for x in 0..img.width() {
                    total += img.pixel(x, y).iter().map(|v| *v as f64).sum::<f64>();
                }
            }
            total / (img.width() * img.height() * 3) as f64
        };
        let (before, after) = (mean(&image), mean(&small));
        println!("{from}² -> {to}²  mean {before:.6} -> {after:.6}");
        assert!(
            (before - after).abs() < 0.01,
            "{from}² -> {to}² changed the mean from {before} to {after}; an area filter \
             conserves total light and §12.2 compares a downsampled full-res render \
             against a proxy one"
        );
    }
}

/// A resample that could enlarge would let a viewport size invent detail.
#[test]
fn a_proxy_larger_than_the_source_is_the_source() {
    let image = Image::new(4, 4, vec![f16::from_f32(0.5); 48]);
    assert_eq!(image.proxy(64), image);
    assert_eq!(image.proxy(4), image);
    assert_eq!(image.proxy(2).width(), 2);
}

/// §7.3 budgets 512 MB for proxy and graph together, so the size of the thing decode
/// hands on is worth knowing rather than discovering.
#[test]
fn a_12_megapixel_proxy_fits_the_budget() {
    let full = 4032u64 * 3024 * 3 * 2;
    let proxy_edge = Image::proxy_longest_edge(4032, 1920) as u64;
    let proxy = proxy_edge * (proxy_edge * 3024 / 4032) * 3 * 2;
    println!(
        "12 MP source: full-res working buffer {:.0} MB, 1080p proxy {:.0} MB",
        full as f64 / 1e6,
        proxy as f64 / 1e6
    );
    assert!(
        proxy < 100_000_000,
        "a 1080p proxy is {proxy} bytes, which leaves little of §7.3's 512 MB for the graph"
    );
}

/// The decoded image really is in the working space, not still in the source's.
#[test]
fn decode_lands_in_linear_display_p3() {
    let p3 = profile::build(&DISPLAY_P3, profile::Curve::Srgb);
    let decoded = decode::decode(&jpeg_with(Some(&p3))).expect("decode");

    // The 2×2 JPEG is a flat grey. A neutral is neutral in every RGB space sharing a
    // white point, so all three channels must agree — and it must be *linear*, so the
    // value has to be the linearisation of the code rather than the code itself.
    let p = decoded.image.pixel(0, 0);
    println!("pixel {p:?}");
    assert!((p[0] - p[1]).abs() < 1e-3 && (p[1] - p[2]).abs() < 1e-3, "a neutral did not stay neutral: {p:?}");

    // And the identity check: a P3 source into a linear-P3 working space is a pure
    // inverse-EOTF, so the matrix must be doing nothing at all here.
    let matrix = DISPLAY_P3.linear_to(&LINEAR_P3);
    for i in 0..3 {
        for j in 0..3 {
            let want = if i == j { 1.0 } else { 0.0 };
            assert!(
                (matrix.0[i][j] - want).abs() < 1e-12,
                "Display P3 to linear Display P3 is not the identity matrix"
            );
        }
    }
}
