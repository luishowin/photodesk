//! EXIF: reading what a photograph says about itself, and deciding what survives an
//! export (§6.1's `metadata` policy).
//!
//! ## Two things the rewriter drops, and why each is a decision
//!
//! **GPS, under `keep-minus-gps`, is removed rather than unreferenced.** Deleting the
//! IFD0 pointer that names the GPS block would make it unreachable while leaving the
//! bytes in the file, which is not what a privacy default means — anything that walks
//! the segment rather than the tree still finds the coordinates. So the TIFF structure
//! is rebuilt from the entries that survive, and what is gone is gone.
//!
//! **The embedded thumbnail is dropped under every policy**, and the reason is
//! stronger than tidiness. IFD1's thumbnail is a picture of the *source*. Carrying it
//! into an export means a file whose preview shows the unedited photograph — and if
//! the edit was a crop, the thumbnail hands back exactly what the crop removed. A
//! stale preview is confusing; a crop that does not crop is a leak.
//!
//! ## Byte order is preserved, deliberately
//!
//! A TIFF block declares its own endianness and every multi-byte value inside follows
//! it. Re-emitting in a fixed order would mean byte-swapping each value according to
//! its type, correctly, for every type — including the ones this module does not
//! otherwise need to understand. Keeping the source's order means the value bytes are
//! copied verbatim and cannot be corrupted by a type this code has never seen.

/// Where a photograph's own idea of "up" lives (EXIF 2.3, tag 0x0112).
pub const ORIENTATION: u16 = 0x0112;
const EXIF_IFD_POINTER: u16 = 0x8769;
const GPS_IFD_POINTER: u16 = 0x8825;
const INTEROP_IFD_POINTER: u16 = 0xA005;

/// The `Exif\0\0` preamble a JPEG's APP1 segment carries before the TIFF block.
pub const APP1_PREFIX: &[u8] = b"Exif\0\0";

#[derive(Clone, Debug, PartialEq)]
pub enum ExifError {
    NotExif(String),
    Malformed(String),
}

impl std::fmt::Display for ExifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifError::NotExif(e) => write!(f, "not an EXIF block: {e}"),
            ExifError::Malformed(e) => write!(f, "malformed EXIF: {e}"),
        }
    }
}

impl std::error::Error for ExifError {}

/// One IFD entry, with its value bytes already resolved out of line.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub tag: u16,
    kind: u16,
    count: u32,
    /// The value's bytes in the *source's* byte order, however many there are.
    value: Vec<u8>,
}

impl Entry {
    /// A SHORT value, if that is what this entry holds.
    pub fn as_u16(&self, big_endian: bool) -> Option<u16> {
        (self.kind == 3 && self.value.len() >= 2).then(|| {
            if big_endian {
                u16::from_be_bytes([self.value[0], self.value[1]])
            } else {
                u16::from_le_bytes([self.value[0], self.value[1]])
            }
        })
    }
}

/// A parsed EXIF block: the directories §6.1's policy has an opinion about.
#[derive(Clone, Debug, PartialEq)]
pub struct Exif {
    pub big_endian: bool,
    ifd0: Vec<Entry>,
    exif: Vec<Entry>,
    gps: Vec<Entry>,
    interop: Vec<Entry>,
}

impl Exif {
    /// The photograph's own idea of "up": 1–8, or 1 when it does not say.
    ///
    /// One rather than "unknown", because EXIF 2.3 makes 1 the default and a file that
    /// says nothing is upright by definition — the alternative is an `Option` every
    /// caller has to resolve to 1 anyway.
    pub fn orientation(&self) -> u8 {
        self.ifd0
            .iter()
            .find(|e| e.tag == ORIENTATION)
            .and_then(|e| e.as_u16(self.big_endian))
            .filter(|v| (1..=8).contains(v))
            .map(|v| v as u8)
            .unwrap_or(1)
    }

    pub fn has_gps(&self) -> bool {
        !self.gps.is_empty()
    }

    /// Entries in IFD0, for reporting. Values are never returned — §6.1's export
    /// default is `keep-minus-gps` for a reason, and a log is not the place to leak a
    /// location.
    pub fn ifd0_tags(&self) -> Vec<u16> {
        self.ifd0.iter().map(|e| e.tag).collect()
    }

    pub fn exif_tags(&self) -> Vec<u16> {
        self.exif.iter().map(|e| e.tag).collect()
    }

    /// Rebuild the block for an export, under `keep_gps`.
    ///
    /// Orientation is written as **1** unconditionally, because by the time anything
    /// is exported the pixels have been turned upright — see `decode`. A file whose
    /// pixels are upright and whose tag says otherwise is rotated twice by anything
    /// that honours the tag.
    pub fn to_bytes(&self, keep_gps: bool) -> Vec<u8> {
        let mut ifd0: Vec<Entry> = self
            .ifd0
            .iter()
            .filter(|e| {
                // The pointers are structure rather than content: they are re-emitted
                // below with offsets this function computes, so carrying the source's
                // would point into a file that no longer exists.
                !matches!(e.tag, EXIF_IFD_POINTER | GPS_IFD_POINTER | INTEROP_IFD_POINTER)
            })
            .cloned()
            .collect();
        ifd0.retain(|e| e.tag != ORIENTATION);
        ifd0.push(Entry {
            tag: ORIENTATION,
            kind: 3,
            count: 1,
            value: if self.big_endian { 1u16.to_be_bytes().to_vec() } else { 1u16.to_le_bytes().to_vec() },
        });
        // IFD entries must be in ascending tag order (TIFF 6.0 §2).
        ifd0.sort_by_key(|e| e.tag);

        let gps = if keep_gps { self.gps.clone() } else { Vec::new() };

        let mut out = Vec::new();
        out.extend_from_slice(if self.big_endian { b"MM" } else { b"II" });
        push_u16(&mut out, 42, self.big_endian);
        push_u32(&mut out, 8, self.big_endian); // IFD0 begins immediately

        // Reserve space for IFD0, then lay the sub-directories and the out-of-line
        // data after it. Sizes are known up front because an IFD is a fixed 2 + 12n + 4.
        let sub_pointers = 1 + usize::from(!self.exif.is_empty())
            + usize::from(!gps.is_empty())
            + usize::from(!self.interop.is_empty());
        let _ = sub_pointers;

        let mut pointers: Vec<(u16, usize)> = Vec::new();
        if !self.exif.is_empty() {
            pointers.push((EXIF_IFD_POINTER, 0));
        }
        if !gps.is_empty() {
            pointers.push((GPS_IFD_POINTER, 0));
        }
        if !self.interop.is_empty() {
            pointers.push((INTEROP_IFD_POINTER, 0));
        }

        let ifd0_len = ifd_size(ifd0.len() + pointers.len());
        let mut cursor = 8 + ifd0_len;

        // Sub-directories, in the order their pointers appear.
        let mut sub_offsets: Vec<(u16, u32, &[Entry])> = Vec::new();
        for (tag, _) in &pointers {
            let entries: &[Entry] = match *tag {
                EXIF_IFD_POINTER => &self.exif,
                GPS_IFD_POINTER => &gps,
                _ => &self.interop,
            };
            sub_offsets.push((*tag, cursor as u32, entries));
            cursor += ifd_size(entries.len()) + data_size(entries);
        }

        // IFD0's own out-of-line data goes last.
        let ifd0_data_at = cursor;

        let mut with_pointers = ifd0.clone();
        for (tag, offset, _) in &sub_offsets {
            with_pointers.push(Entry {
                tag: *tag,
                kind: 4,
                count: 1,
                value: if self.big_endian {
                    offset.to_be_bytes().to_vec()
                } else {
                    offset.to_le_bytes().to_vec()
                },
            });
        }
        with_pointers.sort_by_key(|e| e.tag);

        write_ifd(&mut out, &with_pointers, ifd0_data_at, self.big_endian, 0);
        for (_, _, entries) in &sub_offsets {
            let at = out.len();
            let data_at = at + ifd_size(entries.len());
            write_ifd(&mut out, entries, data_at, self.big_endian, 0);
            for e in *entries {
                if e.value.len() > 4 {
                    out.extend_from_slice(&e.value);
                    if e.value.len() % 2 == 1 {
                        out.push(0);
                    }
                }
            }
        }
        for e in &with_pointers {
            if e.value.len() > 4 {
                out.extend_from_slice(&e.value);
                if e.value.len() % 2 == 1 {
                    out.push(0);
                }
            }
        }
        out
    }
}

fn ifd_size(entries: usize) -> usize {
    2 + entries * 12 + 4
}

fn data_size(entries: &[Entry]) -> usize {
    entries
        .iter()
        .filter(|e| e.value.len() > 4)
        .map(|e| e.value.len() + e.value.len() % 2)
        .sum()
}

fn write_ifd(out: &mut Vec<u8>, entries: &[Entry], mut data_at: usize, big: bool, next: u32) {
    push_u16(out, entries.len() as u16, big);
    for e in entries {
        push_u16(out, e.tag, big);
        push_u16(out, e.kind, big);
        push_u32(out, e.count, big);
        if e.value.len() <= 4 {
            let mut padded = e.value.clone();
            padded.resize(4, 0);
            out.extend_from_slice(&padded);
        } else {
            push_u32(out, data_at as u32, big);
            data_at += e.value.len() + e.value.len() % 2;
        }
    }
    push_u32(out, next, big);
}

fn push_u16(out: &mut Vec<u8>, v: u16, big: bool) {
    out.extend_from_slice(&if big { v.to_be_bytes() } else { v.to_le_bytes() });
}

fn push_u32(out: &mut Vec<u8>, v: u32, big: bool) {
    out.extend_from_slice(&if big { v.to_be_bytes() } else { v.to_le_bytes() });
}

/// Parse a TIFF block — the bytes after `Exif\0\0` in a JPEG's APP1 segment.
///
/// Every read is bounds-checked and every failure is an error. This arrives from a
/// file somebody else wrote, and a panic here is a crash on opening a photograph.
pub fn parse(tiff: &[u8]) -> Result<Exif, ExifError> {
    if tiff.len() < 8 {
        return Err(ExifError::NotExif(format!("{} bytes", tiff.len())));
    }
    let big_endian = match &tiff[0..2] {
        b"MM" => true,
        b"II" => false,
        other => {
            return Err(ExifError::NotExif(format!(
                "byte order is `{}`, not II or MM",
                String::from_utf8_lossy(other)
            )));
        }
    };
    let magic = read_u16(tiff, 2, big_endian)?;
    if magic != 42 {
        return Err(ExifError::NotExif(format!("magic is {magic}, not 42")));
    }
    let ifd0_at = read_u32(tiff, 4, big_endian)? as usize;

    let ifd0 = read_ifd(tiff, ifd0_at, big_endian)?;
    let sub = |tag: u16| -> Result<Vec<Entry>, ExifError> {
        match ifd0.iter().find(|e| e.tag == tag) {
            Some(e) if e.value.len() >= 4 => {
                let at = if big_endian {
                    u32::from_be_bytes([e.value[0], e.value[1], e.value[2], e.value[3]])
                } else {
                    u32::from_le_bytes([e.value[0], e.value[1], e.value[2], e.value[3]])
                } as usize;
                read_ifd(tiff, at, big_endian)
            }
            _ => Ok(Vec::new()),
        }
    };

    Ok(Exif {
        exif: sub(EXIF_IFD_POINTER)?,
        gps: sub(GPS_IFD_POINTER)?,
        interop: sub(INTEROP_IFD_POINTER)?,
        ifd0,
        big_endian,
    })
}

/// Find and parse a JPEG's APP1 EXIF block.
pub fn from_jpeg(bytes: &[u8]) -> Option<Exif> {
    let mut i = 2usize; // past SOI
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
            break;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let segment = bytes.get(i + 4..i + 2 + len)?;
        if marker == 0xE1 && segment.starts_with(APP1_PREFIX) {
            return parse(&segment[APP1_PREFIX.len()..]).ok();
        }
        i += 2 + len;
    }
    None
}

fn read_ifd(tiff: &[u8], at: usize, big: bool) -> Result<Vec<Entry>, ExifError> {
    let count = read_u16(tiff, at, big)? as usize;
    // A count that does not fit is a malformed file, not a reason to read past the end.
    if at + 2 + count * 12 > tiff.len() {
        return Err(ExifError::Malformed(format!(
            "an IFD claims {count} entries, which does not fit in {} bytes",
            tiff.len()
        )));
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let e = at + 2 + i * 12;
        let tag = read_u16(tiff, e, big)?;
        let kind = read_u16(tiff, e + 2, big)?;
        let n = read_u32(tiff, e + 4, big)?;
        let unit = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            // A type this code has never seen. Kept with its four inline bytes rather
            // than dropped: an unknown tag is somebody's metadata, and §6.1's policy
            // is about GPS rather than about what this module happens to understand.
            _ => 1,
        };
        let len = (n as usize).saturating_mul(unit);
        let value = if len <= 4 {
            tiff.get(e + 8..e + 12)
                .ok_or_else(|| ExifError::Malformed("a truncated entry".into()))?[..len.min(4)]
                .to_vec()
        } else {
            let at = read_u32(tiff, e + 8, big)? as usize;
            tiff.get(at..at + len)
                .ok_or_else(|| {
                    ExifError::Malformed(format!("tag {tag:#06x} points past the end"))
                })?
                .to_vec()
        };
        entries.push(Entry { tag, kind, count: n, value });
    }
    Ok(entries)
}

fn read_u16(b: &[u8], at: usize, big: bool) -> Result<u16, ExifError> {
    let s = b
        .get(at..at + 2)
        .ok_or_else(|| ExifError::Malformed(format!("truncated at {at}")))?;
    Ok(if big {
        u16::from_be_bytes([s[0], s[1]])
    } else {
        u16::from_le_bytes([s[0], s[1]])
    })
}

fn read_u32(b: &[u8], at: usize, big: bool) -> Result<u32, ExifError> {
    let s = b
        .get(at..at + 4)
        .ok_or_else(|| ExifError::Malformed(format!("truncated at {at}")))?;
    Ok(if big {
        u32::from_be_bytes([s[0], s[1], s[2], s[3]])
    } else {
        u32::from_le_bytes([s[0], s[1], s[2], s[3]])
    })
}
