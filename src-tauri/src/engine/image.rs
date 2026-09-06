//! An image in the working space: linear Display P3, f16 (§4, frozen).
//!
//! The one thing worth reading twice is [`Image::resample`]. §7.1 puts a downsample
//! between decode and the shader chain — "decode once to full-res linear, then proxy"
//! — and averaging pixels is only correct in **linear light**, which is why the
//! downsample lives on this side of the colour conversion rather than on the decoder's.
//!
//! Averaging in an encoded space is the classic version of this bug, and it is not
//! subtle: two adjacent 8-bit sRGB pixels at 0 and 255 average to 128 encoded, which
//! is 21.6% linear — where the right answer is 50% linear, or code 188. A thumbnail
//! made that way is visibly too dark, and the darkening tracks local contrast, so it
//! looks like a bad photograph rather than a bad resampler.

use half::f16;

/// Pixels in the working space. Three channels, interleaved, no alpha.
///
/// No alpha because §1's subjects do not have one — a photograph is opaque, and §14's
/// v0.1 has nothing that would introduce transparency. When something does (a mask
/// preview, a composite over a checkerboard) it will be a separate channel with a
/// stated meaning rather than a fourth number that has always been 1.0.
#[derive(Clone, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<f16>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image {}×{} linear P3 f16 ({:.1} MB)",
            self.width,
            self.height,
            self.pixels.len() as f64 * 2.0 / 1_048_576.0
        )
    }
}

impl Image {
    /// `pixels` is `width × height × 3` values of linear Display P3.
    pub fn new(width: u32, height: u32, pixels: Vec<f16>) -> Self {
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 3,
            "an image's buffer must be exactly width × height × 3"
        );
        Self { width, height, pixels }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[f16] {
        &self.pixels
    }

    /// Bytes of working-buffer storage, which §7.3 caps at 512 MB for proxy and graph
    /// together.
    pub fn bytes(&self) -> usize {
        self.pixels.len() * 2
    }

    pub fn pixel(&self, x: u32, y: u32) -> [f32; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [
            self.pixels[i].to_f32(),
            self.pixels[i + 1].to_f32(),
            self.pixels[i + 2].to_f32(),
        ]
    }

    /// §7.1's proxy size: `min(2 × viewport_longest_edge, source_longest_edge)`.
    ///
    /// The `min` is what stops a proxy being an upscale. Editing a 900-pixel scan on a
    /// 4K display would otherwise produce a "proxy" four times the size of the source,
    /// which costs memory and adds nothing — there is no detail up there to see.
    pub fn proxy_longest_edge(source_longest_edge: u32, viewport_longest_edge: u32) -> u32 {
        (2 * viewport_longest_edge).min(source_longest_edge).max(1)
    }

    /// Downsample so the longest edge is `longest_edge`, preserving aspect ratio.
    ///
    /// Returns `self` unchanged when it is already that size or smaller: §7.1's proxy
    /// is a *reduction*, and a resample that could enlarge would let a viewport size
    /// silently invent detail.
    pub fn proxy(&self, longest_edge: u32) -> Image {
        let source_longest = self.width.max(self.height);
        if longest_edge >= source_longest {
            return self.clone();
        }
        let scale = longest_edge as f64 / source_longest as f64;
        let w = ((self.width as f64 * scale).round() as u32).max(1);
        let h = ((self.height as f64 * scale).round() as u32).max(1);
        self.resample(w, h)
    }

    /// Exact area average, in linear light.
    ///
    /// Each destination pixel is the mean of the source over its exact footprint,
    /// including the partial coverage at the edges — so the result does not depend on
    /// the ratio being an integer, which is the case that a naive box filter gets
    /// subtly wrong and a nearest-neighbour one gets obviously wrong.
    ///
    /// "The library default is correct until a golden-image test says otherwise" is
    /// §3's rule for demosaic, and it applies here for the same reason: a resampling
    /// filter is a picture-quality decision, and §12.1 is where such a decision gets
    /// re-opened with evidence rather than by preference. Area-average is the default
    /// with the fewest surprises — no ringing, no halos, and exactly conservative of
    /// total light, which matters because §12.2 compares a full-res render downsampled
    /// against a proxy render and any energy the filter invents shows up there.
    pub fn resample(&self, width: u32, height: u32) -> Image {
        assert!(width > 0 && height > 0, "an image cannot have a zero side");
        let sx = self.width as f64 / width as f64;
        let sy = self.height as f64 / height as f64;
        let mut out = vec![f16::ZERO; width as usize * height as usize * 3];

        for y in 0..height {
            let (y0, y1) = (y as f64 * sy, (y as f64 + 1.0) * sy);
            let rows = coverage(y0, y1, self.height);
            for x in 0..width {
                let (x0, x1) = (x as f64 * sx, (x as f64 + 1.0) * sx);
                let cols = coverage(x0, x1, self.width);

                let mut acc = [0.0f64; 3];
                let mut total = 0.0f64;
                for &(row, wy) in &rows {
                    let base = row * self.width as usize;
                    for &(col, wx) in &cols {
                        let weight = wy * wx;
                        let i = (base + col) * 3;
                        acc[0] += self.pixels[i].to_f32() as f64 * weight;
                        acc[1] += self.pixels[i + 1].to_f32() as f64 * weight;
                        acc[2] += self.pixels[i + 2].to_f32() as f64 * weight;
                        total += weight;
                    }
                }
                let o = (y as usize * width as usize + x as usize) * 3;
                for c in 0..3 {
                    out[o + c] = f16::from_f64(acc[c] / total);
                }
            }
        }
        Image::new(width, height, out)
    }
}

/// Which source pixels the interval `[lo, hi)` covers, and by how much.
fn coverage(lo: f64, hi: f64, extent: u32) -> Vec<(usize, f64)> {
    let first = lo.floor().max(0.0) as usize;
    let last = ((hi.ceil() as usize).min(extent as usize)).max(first + 1);
    (first..last)
        .map(|i| {
            let overlap = (hi.min(i as f64 + 1.0) - lo.max(i as f64)).max(0.0);
            (i, overlap)
        })
        .filter(|(_, w)| *w > 0.0)
        .collect()
}
