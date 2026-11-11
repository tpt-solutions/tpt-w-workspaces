// Copyright (c) 2026 TPT Solutions
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conversion between desktop GUIDs and their `{XXXXXXXX-...}` string form.

use windows::core::GUID;

/// Formats a GUID as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` (uppercase).
pub fn format(g: &GUID) -> String {
    let d = g.data4;
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

/// Parses the string produced by [`format`]. Braces are required; hex digits may be either case.
pub fn parse(s: &str) -> Option<GUID> {
    if !s.is_ascii() {
        return None;
    }
    let inner = s.strip_prefix('{')?.strip_suffix('}')?;
    let parts: Vec<&str> = inner.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    let lengths = [8, 4, 4, 4, 12];
    if parts.iter().zip(lengths).any(|(p, n)| p.len() != n) {
        return None;
    }
    let data1 = u32::from_str_radix(parts[0], 16).ok()?;
    let data2 = u16::from_str_radix(parts[1], 16).ok()?;
    let data3 = u16::from_str_radix(parts[2], 16).ok()?;
    let tail = format!("{}{}", parts[3], parts[4]);
    let mut data4 = [0u8; 8];
    for (i, byte) in data4.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&tail[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(GUID {
        data1,
        data2,
        data3,
        data4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let g = GUID {
            data1: 0x9F3A_1B2C,
            data2: 0x1234,
            data3: 0xABCD,
            data4: [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF],
        };
        let s = format(&g);
        assert_eq!(s, "{9F3A1B2C-1234-ABCD-0123-456789ABCDEF}");
        assert_eq!(parse(&s), Some(g));
        assert_eq!(parse(&s.to_lowercase()), Some(g));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse("9F3A1B2C-1234-ABCD-0123-456789ABCDEF"), None);
        assert_eq!(parse("{9F3A1B2C-1234-ABCD-0123}"), None);
        assert_eq!(parse("{9F3A1B2C-1234-ABCD-0123-456789ABCDEG}"), None);
        assert_eq!(parse("{ÿ}"), None);
        assert_eq!(parse(""), None);
    }
}
