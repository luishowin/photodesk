//! What can this machine actually do with a HEIF container?
//!
//! §1 names an iPhone HEIF as the native subject and §14 makes v0.1 literally "open an
//! iPhone HEIF". An iPhone HEIC is HEVC-coded, and Fedora ships libheif without linking
//! x265 or libde265 — so whether the target platform can open the target format is a
//! v0.1-blocking question, and it is cheaper to answer here than in week three.
//!
//! This test enumerates rather than asserts a particular outcome: the machine's codec
//! set is an environment fact, and the useful thing is to have it written down.

use libheif_rs::{CompressionFormat, LibHeif};

#[test]
fn report_available_heif_codecs() {
    let lh = LibHeif::new();
    let v = lh.version();
    println!("libheif {}.{}.{}", v[0], v[1], v[2]);

    let formats = [
        ("HEVC (iPhone HEIC)", CompressionFormat::Hevc),
        ("AV1 (AVIF)", CompressionFormat::Av1),
        ("AVC (H.264)", CompressionFormat::Avc),
        ("JPEG", CompressionFormat::Jpeg),
        ("JPEG 2000", CompressionFormat::Jpeg2000),
        ("uncompressed", CompressionFormat::Uncompressed),
    ];

    println!("\n{:<22} {:<38} {}", "format", "decoders", "encoders");
    let mut hevc_decode = false;
    for (label, fmt) in formats {
        let dec: Vec<String> = lh
            .decoder_descriptors(16, Some(fmt))
            .iter()
            .map(|d| d.name())
            .collect();
        let enc: Vec<String> = lh
            .encoder_descriptors(16, Some(fmt), None)
            .iter()
            .map(|e| e.name())
            .collect();
        if matches!(fmt, CompressionFormat::Hevc) {
            hevc_decode = !dec.is_empty();
        }
        println!(
            "{:<22} {:<38} {}",
            label,
            if dec.is_empty() { "—".to_string() } else { dec.join(", ") },
            if enc.is_empty() { "—".to_string() } else { enc.join(", ") }
        );
    }

    println!("\nHEVC decode available: {hevc_decode}  <- the one v0.1 depends on");

    // Asserted rather than reported, now that the requirement is documented (§3, §13).
    // Fedora ships libheif without HEVC on patent grounds and it is not obvious from
    // any error the app would otherwise produce: an iPhone HEIC simply fails to open.
    // A red test here is the cheapest possible way to say "your machine is not set up".
    assert!(
        hevc_decode,
        "no HEVC decoder: this machine cannot open an iPhone HEIC, which is §1's native \
         subject and the whole of §14's v0.1. Fedora ships libheif without HEVC on \
         patent grounds; install RPM Fusion's `libheif-freeworld`."
    );
}
