//! Reconstruct the contour of a completed hard flood, never its connectivity.
//! A 3×3 neighborhood determines sixteen bilinear subpixel tests. The lookup
//! keeps that work constant per output pixel, with no enlarged raster or mask.

use crate::preview::PixelRect;

pub(super) const ACCEPTED: u8 = 2;
const COVERAGE: [u8; 512] = coverage_table();

const fn coverage_table() -> [u8; 512] {
    let mut table = [0; 512];
    let mut pattern = 0;
    while pattern < table.len() {
        let mut samples = 0;
        let mut sy = 0;
        while sy < 4 {
            let y = if sy < 2 { 0 } else { 1 };
            let wy = [5, 7, 1, 3][sy];
            let mut sx = 0;
            while sx < 4 {
                let x = if sx < 2 { 0 } else { 1 };
                let wx = [5, 7, 1, 3][sx];
                let mut value = 0;
                let mut dy = 0;
                while dy < 2 {
                    let mut dx = 0;
                    while dx < 2 {
                        if pattern & (1 << ((y + dy) * 3 + x + dx)) != 0 {
                            value += (if dx == 0 { 8 - wx } else { wx })
                                * (if dy == 0 { 8 - wy } else { wy });
                        }
                        dx += 1;
                    }
                    dy += 1;
                }
                if value >= 32 {
                    samples += 1;
                }
                sx += 1;
            }
            sy += 1;
        }
        table[pattern] = ((samples * 255 + 8) / 16) as u8;
        pattern += 1;
    }
    table
}

pub(super) fn apply(
    pixels: &mut [u8],
    visited: &[u8],
    source: (u32, u32),
    local_bounds: PixelRect,
    included: impl Fn(u32, u32) -> bool,
) {
    let (width, height) = source;
    for row in 0..local_bounds.height {
        let y = local_bounds.y as u32 + row;
        let ys = [y.saturating_sub(1), y, (y + 1).min(height - 1)];
        for column in 0..local_bounds.width {
            let x = local_bounds.x as u32 + column;
            let index = (row * local_bounds.width + column) as usize;
            // Limits and sealed gap barriers stay strict, even at a soft edge.
            if !included(x, y) {
                pixels[index] = 0;
                continue;
            }
            let xs = [x.saturating_sub(1), x, (x + 1).min(width - 1)];
            let mut pattern = 0;
            for (dy, py) in ys.into_iter().enumerate() {
                for (dx, px) in xs.into_iter().enumerate() {
                    if visited[(py * width + px) as usize] == ACCEPTED {
                        pattern |= 1 << (dy * 3 + dx);
                    }
                }
            }
            pixels[index] = COVERAGE[pattern];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_preserves_symmetry_complements_and_full_or_empty_interiors() {
        assert_eq!(COVERAGE[0], 0);
        assert_eq!(COVERAGE[511], 255);
        assert_eq!(COVERAGE[1 << 4], 191);
        for pattern in 0..512 {
            let mut rotated = 0;
            let mut reflected = 0;
            for y in 0..3 {
                for x in 0..3 {
                    if pattern & (1 << (y * 3 + x)) != 0 {
                        rotated |= 1 << (x * 3 + 2 - y);
                        reflected |= 1 << (y * 3 + 2 - x);
                    }
                }
            }
            assert_eq!(COVERAGE[pattern], COVERAGE[rotated]);
            assert_eq!(COVERAGE[pattern], COVERAGE[reflected]);
            assert!(
                u16::from(COVERAGE[pattern]).abs_diff(255 - u16::from(COVERAGE[511 ^ pattern]))
                    <= 1
            );
        }
    }
}
