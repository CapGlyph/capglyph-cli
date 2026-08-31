//! Byte interleave / de-interleave (QR-style, crop is burst error).
//!
//! Interleaving spreads burst errors (crop, scratch) across multiple codewords
//! so that Reed-Solomon / BCH can correct them. Depth = number of parallel
//! codewords interleaved. Depth 0 or 1 is no-op (for compat).

/// Interleave `bytes` with `depth` (1 = no-op).
/// QR-style: `out[ i*depth + (i % depth?) ]` — here we implement block interleave:
/// split input into `depth` columns (ceil), then read row-wise.
/// Example: depth=3, input=[a0,a1,a2,a3,a4,a5,a6] → columns [[a0,a3,a6],[a1,a4],[a2,a5]] → read rows → [a0,a1,a2,a3,a4,a5,a6] for already small… better illustrate: 12 bytes, depth=4 → [0..12] → out=[0,3,6,9,1,4,7,10,2,5,8,11].
pub fn interleave(bytes: &[u8], depth: u8) -> Vec<u8> {
    let d = depth as usize;
    if d <= 1 || bytes.is_empty() {
        return bytes.to_vec();
    }
    let n = bytes.len();
    let rows = n.div_ceil(d);
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        for c in 0..d {
            let idx = c * rows + r;
            if idx < n {
                out.push(bytes[idx]);
            }
        }
    }
    out
}

/// Inverse of `interleave`.
pub fn deinterleave(bytes: &[u8], depth: u8) -> Vec<u8> {
    let d = depth as usize;
    if d <= 1 || bytes.is_empty() {
        return bytes.to_vec();
    }
    let n = bytes.len();
    let rows = n.div_ceil(d);
    // Reconstruct original column-major order.
    let mut out = vec![0u8; n];
    let mut pos = 0usize;
    for r in 0..rows {
        for c in 0..d {
            let idx = c * rows + r;
            if idx < n {
                out[idx] = bytes[pos];
                pos += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleave_roundtrip() {
        let data: Vec<u8> = (0..32).collect();
        for depth in [0u8, 1, 2, 4, 8, 16] {
            let enc = interleave(&data, depth);
            let dec = deinterleave(&enc, depth);
            assert_eq!(dec, data, "depth {}", depth);
        }
    }

    #[test]
    fn interleave_spreads_burst() {
        let data: Vec<u8> = (0..12).collect();
        let enc = interleave(&data, 4);
        assert_eq!(enc, vec![0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11]);
        let dec = deinterleave(&enc, 4);
        assert_eq!(dec, data);
    }

    #[test]
    fn empty_identity() {
        assert_eq!(interleave(&[], 8), Vec::<u8>::new());
        assert_eq!(deinterleave(&[], 8), Vec::<u8>::new());
    }
}
