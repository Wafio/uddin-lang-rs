//! Addresses and networks for `waf_cidr_match`, read and compared as Go's `net` package does: an address is four
//! bytes or sixteen, and one that is an IPv4 address written in IPv6 form (`::ffff:10.0.0.1`) is the IPv4 address.

use std::net::Ipv6Addr;

/// An IPv4 address of four decimal fields without leading zeros, or an IPv6 address; sixteen bytes either way.
fn parse_ip(text: &[u8]) -> Option<[u8; 16]> {
    let text = std::str::from_utf8(text).ok()?;
    if text.contains(':') {
        return text.parse::<Ipv6Addr>().ok().map(|a| a.octets());
    }
    let four = parse_v4(text)?;
    let mut out = [0u8; 16];
    out[10] = 0xFF;
    out[11] = 0xFF;
    out[12..].copy_from_slice(&four);
    Some(out)
}

fn parse_v4(text: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut fields = text.split('.');
    for slot in &mut out {
        let f = fields.next()?;
        if f.is_empty() || f.len() > 3 || !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if f.len() > 1 && f.starts_with('0') {
            return None;
        }
        *slot = f.parse::<u16>().ok().and_then(|n| u8::try_from(n).ok())?;
    }
    fields.next().is_none().then_some(out)
}

fn is_v4(ip: &[u8; 16]) -> bool {
    ip[..10].iter().all(|&b| b == 0) && ip[10] == 0xFF && ip[11] == 0xFF
}

/// A network: the address with the host bits cleared, and the mask. Four bytes of each for an IPv4 network.
struct Network {
    addr: Vec<u8>,
    mask: Vec<u8>,
}

fn mask_of(bits: usize, len_bits: usize) -> Vec<u8> {
    (0..len_bits / 8)
        .map(|i| match bits.saturating_sub(i * 8) {
            0 => 0,
            n if n >= 8 => 0xFF,
            n => 0xFFu8 << (8 - n),
        })
        .collect()
}

fn parse_cidr(text: &[u8]) -> Option<Network> {
    let slash = text.iter().position(|&b| b == b'/')?;
    let (addr, bits) = (&text[..slash], &text[slash + 1..]);
    let ip = parse_ip(addr)?;
    let in_v4_text = !addr.contains(&b':');
    let (len_bits, bytes): (usize, Vec<u8>) = if in_v4_text {
        (32, ip[12..].to_vec())
    } else {
        (128, ip.to_vec())
    };
    if bits.is_empty() || !bits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut n = 0usize;
    for &d in bits {
        n = n * 10 + usize::from(d - b'0');
        if n > len_bits {
            return None;
        }
    }
    let mask = mask_of(n, len_bits);
    let addr = bytes.iter().zip(&mask).map(|(a, m)| a & m).collect();
    Some(Network { addr, mask })
}

/// Whether `ip` is in `cidr`; text that is not an address or a network is in nothing.
pub fn contains(cidr: &[u8], ip: &[u8]) -> bool {
    let (Some(ip), Some(net)) = (parse_ip(ip), parse_cidr(cidr)) else {
        return false;
    };
    let candidate: Vec<u8> = if is_v4(&ip) {
        ip[12..].to_vec()
    } else {
        ip.to_vec()
    };
    // A network whose address is an IPv4 address in IPv6 form is the IPv4 network, with the last four bytes of
    // its mask.
    let (mut addr, mut mask) = (net.addr, net.mask);
    if addr.len() == 16 {
        let mut full = [0u8; 16];
        full.copy_from_slice(&addr);
        if is_v4(&full) {
            addr = full[12..].to_vec();
            if mask.len() == 16 {
                mask = mask[12..].to_vec();
            }
        }
    }
    if addr.len() != mask.len() || addr.len() != candidate.len() {
        return false;
    }
    addr.iter()
        .zip(&mask)
        .zip(&candidate)
        .all(|((n, m), c)| n & m == c & m)
}

#[cfg(test)]
mod tests {
    use super::contains;

    #[test]
    fn compares_addresses_with_networks() {
        let yes = [
            ("10.0.0.0/8", "10.1.2.3"),
            ("192.168.1.0/24", "192.168.1.255"),
            ("0.0.0.0/0", "8.8.8.8"),
            ("2001:db8::/32", "2001:db8::1"),
            ("::/0", "2001:db8::1"),
            ("10.0.0.0/8", "::ffff:10.0.0.1"),
            ("::ffff:10.0.0.0/104", "10.9.9.9"),
            ("1.2.3.4/32", "1.2.3.4"),
            ("10.0.0.0/08", "10.0.0.1"),
        ];
        for (c, i) in yes {
            assert!(contains(c.as_bytes(), i.as_bytes()), "{c} {i}");
        }
        let no = [
            ("10.0.0.0/8", "11.0.0.1"),
            ("10.0.0.0/8", "nope"),
            ("nope", "10.0.0.1"),
            ("10.0.0.0", "10.0.0.1"),
            ("10.0.0.0/33", "10.0.0.1"),
            ("010.0.0.0/8", "10.0.0.1"),
            ("10.0.0.0/8", "10.0.0.256"),
            ("2001:db8::/32", "10.0.0.1"),
            ("10.0.0.0/8", "2001:db8::1"),
            ("fe80::/10", "fe80::1%eth0"),
        ];
        for (c, i) in no {
            assert!(!contains(c.as_bytes(), i.as_bytes()), "{c} {i}");
        }
    }
}
