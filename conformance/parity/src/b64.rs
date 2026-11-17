//! Standard base64 with padding, for sources that are not valid UTF-8.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * k) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn decode(text: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("length is not a multiple of 4");
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for quad in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &c in quad {
            let v = if c == b'=' {
                pad += 1;
                0
            } else {
                ALPHABET
                    .iter()
                    .position(|&a| a == c)
                    .ok_or("bad character")? as u32
            };
            n = n << 6 | v;
        }
        if pad > 2 {
            return Err("bad padding");
        }
        let all = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&all[..3 - pad]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for len in 0..40u8 {
            let data: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(decode(&encode(&data)).as_deref(), Ok(&data[..]));
        }
        assert_eq!(encode(b"x = 1\n"), "eCA9IDEK");
    }
}
