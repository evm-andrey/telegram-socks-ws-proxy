use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;
use once_cell::sync::Lazy;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub type Aes256Ctr = ctr::Ctr128BE<Aes256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    Telegram,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct TelegramInitInfo {
    pub dc: Option<u8>,
    pub is_media: bool,
}

#[derive(Debug, Clone)]
pub struct TelegramIpEntry {
    pub dc: u8,
    pub is_media: bool,
}

pub struct MtProtoMessageSplitter {
    cipher: Aes256Ctr,
}

pub static IP_TO_DC: Lazy<Vec<(u32, u32, TelegramIpEntry)>> = Lazy::new(|| {
    vec![
        (
            ip_to_u32("149.154.175.50"),
            ip_to_u32("149.154.175.50"),
            TelegramIpEntry {
                dc: 1,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.51"),
            ip_to_u32("149.154.175.51"),
            TelegramIpEntry {
                dc: 1,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.53"),
            ip_to_u32("149.154.175.53"),
            TelegramIpEntry {
                dc: 1,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.54"),
            ip_to_u32("149.154.175.54"),
            TelegramIpEntry {
                dc: 1,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.52"),
            ip_to_u32("149.154.175.52"),
            TelegramIpEntry {
                dc: 1,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.220"),
            ip_to_u32("149.154.167.220"),
            TelegramIpEntry {
                dc: 2,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.167.41"),
            ip_to_u32("149.154.167.41"),
            TelegramIpEntry {
                dc: 2,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.167.50"),
            ip_to_u32("149.154.167.50"),
            TelegramIpEntry {
                dc: 2,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.167.51"),
            ip_to_u32("149.154.167.51"),
            TelegramIpEntry {
                dc: 2,
                is_media: false,
            },
        ),
        (
            ip_to_u32("95.161.76.100"),
            ip_to_u32("95.161.76.100"),
            TelegramIpEntry {
                dc: 2,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.167.35"),
            ip_to_u32("149.154.167.35"),
            TelegramIpEntry {
                dc: 2,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.222"),
            ip_to_u32("149.154.167.222"),
            TelegramIpEntry {
                dc: 2,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.151"),
            ip_to_u32("149.154.167.151"),
            TelegramIpEntry {
                dc: 2,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.223"),
            ip_to_u32("149.154.167.223"),
            TelegramIpEntry {
                dc: 2,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.162.123"),
            ip_to_u32("149.154.162.123"),
            TelegramIpEntry {
                dc: 2,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.175.100"),
            ip_to_u32("149.154.175.100"),
            TelegramIpEntry {
                dc: 3,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.101"),
            ip_to_u32("149.154.175.101"),
            TelegramIpEntry {
                dc: 3,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.175.102"),
            ip_to_u32("149.154.175.102"),
            TelegramIpEntry {
                dc: 3,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.91"),
            ip_to_u32("149.154.167.91"),
            TelegramIpEntry {
                dc: 4,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.167.92"),
            ip_to_u32("149.154.167.92"),
            TelegramIpEntry {
                dc: 4,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.164.250"),
            ip_to_u32("149.154.164.250"),
            TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.166.120"),
            ip_to_u32("149.154.166.120"),
            TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.166.121"),
            ip_to_u32("149.154.166.121"),
            TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.167.118"),
            ip_to_u32("149.154.167.118"),
            TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        ),
        (
            ip_to_u32("149.154.165.111"),
            ip_to_u32("149.154.165.111"),
            TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        ),
        (
            ip_to_u32("91.108.56.100"),
            ip_to_u32("91.108.56.100"),
            TelegramIpEntry {
                dc: 5,
                is_media: false,
            },
        ),
        (
            ip_to_u32("91.108.56.101"),
            ip_to_u32("91.108.56.101"),
            TelegramIpEntry {
                dc: 5,
                is_media: false,
            },
        ),
        (
            ip_to_u32("91.108.56.116"),
            ip_to_u32("91.108.56.116"),
            TelegramIpEntry {
                dc: 5,
                is_media: false,
            },
        ),
        (
            ip_to_u32("91.108.56.126"),
            ip_to_u32("91.108.56.126"),
            TelegramIpEntry {
                dc: 5,
                is_media: false,
            },
        ),
        (
            ip_to_u32("149.154.171.5"),
            ip_to_u32("149.154.171.5"),
            TelegramIpEntry {
                dc: 5,
                is_media: false,
            },
        ),
        (
            ip_to_u32("91.108.56.102"),
            ip_to_u32("91.108.56.102"),
            TelegramIpEntry {
                dc: 5,
                is_media: true,
            },
        ),
        (
            ip_to_u32("91.108.56.128"),
            ip_to_u32("91.108.56.128"),
            TelegramIpEntry {
                dc: 5,
                is_media: true,
            },
        ),
        (
            ip_to_u32("91.108.56.151"),
            ip_to_u32("91.108.56.151"),
            TelegramIpEntry {
                dc: 5,
                is_media: true,
            },
        ),
    ]
});

pub static TG_RANGES: Lazy<Vec<(u32, u32, AddressKind)>> = Lazy::new(|| {
    vec![
        (
            ip_to_u32("185.76.151.0"),
            ip_to_u32("185.76.151.255"),
            AddressKind::Telegram,
        ),
        (
            ip_to_u32("149.154.160.0"),
            ip_to_u32("149.154.175.255"),
            AddressKind::Telegram,
        ),
        (
            ip_to_u32("91.105.192.0"),
            ip_to_u32("91.105.193.255"),
            AddressKind::Telegram,
        ),
        (
            ip_to_u32("91.108.0.0"),
            ip_to_u32("91.108.255.255"),
            AddressKind::Telegram,
        ),
    ]
});

pub fn is_telegram_ip(ip: &str) -> bool {
    let Ok(ip) = Ipv4Addr::from_str(ip) else {
        return false;
    };
    let value = u32::from(ip);
    TG_RANGES.iter().any(|(start, end, kind)| {
        *kind == AddressKind::Telegram && value >= *start && value <= *end
    })
}

pub fn ip_to_dc(ip: &str) -> Option<TelegramIpEntry> {
    let parsed = Ipv4Addr::from_str(ip).ok()?;
    let value = u32::from(parsed);
    for (start, end, entry) in IP_TO_DC.iter() {
        if value >= *start && value <= *end {
            return Some(entry.clone());
        }
    }
    None
}

pub fn extract_dc(data: &[u8]) -> TelegramInitInfo {
    if data.len() < 64 {
        return TelegramInitInfo {
            dc: None,
            is_media: false,
        };
    }

    let key = &data[8..40];
    let iv = &data[40..56];
    let mut cipher = Aes256Ctr::new_from_slices(key, iv).ok();
    let mut stream = [0u8; 64];
    if let Some(ref mut c) = cipher {
        c.apply_keystream(&mut stream);
    }
    let plain = {
        let mut p = [0u8; 8];
        for i in 0..8 {
            p[i] = data[56 + i] ^ stream[56 + i];
        }
        p
    };

    let proto = u32::from_le_bytes(plain[0..4].try_into().unwrap_or([0u8; 4]));
    if !matches!(proto, 0xEFEFEFEF | 0xEEEEEEEE | 0xDDDDDDDD) {
        return TelegramInitInfo {
            dc: None,
            is_media: false,
        };
    }

    let dc_raw = i16::from_le_bytes(plain[4..6].try_into().unwrap_or([0u8; 2]));
    Some(dc_raw)
        .map(|v| v.abs() as u8)
        .and_then(|dc| {
            if (1..=5).contains(&dc) {
                Some(TelegramInitInfo {
                    dc: Some(dc),
                    is_media: dc_raw < 0,
                })
            } else {
                None
            }
        })
        .unwrap_or(TelegramInitInfo {
            dc: None,
            is_media: false,
        })
}

pub fn patch_init_dc(data: &[u8], dc: i16) -> Vec<u8> {
    if data.len() < 64 {
        return data.to_vec();
    }

    let key = &data[8..40];
    let iv = &data[40..56];
    let mut cipher = match Aes256Ctr::new_from_slices(key, iv) {
        Ok(c) => c,
        Err(_) => return data.to_vec(),
    };
    let mut stream = [0u8; 64];
    cipher.apply_keystream(&mut stream);
    let mut out = data[..64].to_vec();
    let dc_bytes = dc.to_le_bytes();
    out[60] = stream[60] ^ dc_bytes[0];
    out[61] = stream[61] ^ dc_bytes[1];
    if data.len() > 64 {
        out.extend_from_slice(&data[64..]);
    }
    out
}

pub fn ws_domains(dc: u8, is_media: bool) -> Vec<String> {
    if is_media {
        vec![
            format!("kws{dc}-1.web.telegram.org"),
            format!("kws{dc}.web.telegram.org"),
        ]
    } else {
        vec![
            format!("kws{dc}.web.telegram.org"),
            format!("kws{dc}-1.web.telegram.org"),
        ]
    }
}

impl MtProtoMessageSplitter {
    pub fn new(init_data: &[u8]) -> Option<Self> {
        if init_data.len() < 64 {
            return None;
        }

        let key = &init_data[8..40];
        let iv = &init_data[40..56];
        let mut cipher = Aes256Ctr::new_from_slices(key, iv).ok()?;

        let mut skip = [0u8; 64];
        cipher.apply_keystream(&mut skip);

        Some(Self { cipher })
    }

    pub fn split(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        if chunk.is_empty() {
            return vec![Vec::new()];
        }

        let mut plain = chunk.to_vec();
        self.cipher.apply_keystream(&mut plain);

        let mut boundaries = Vec::new();
        let mut pos = 0usize;
        while pos < plain.len() {
            let first = plain[pos];
            let msg_len = if first == 0x7f {
                if pos + 4 > plain.len() {
                    break;
                }
                let raw_len = (plain[pos + 1] as usize)
                    | ((plain[pos + 2] as usize) << 8)
                    | ((plain[pos + 3] as usize) << 16);
                pos += 4;
                raw_len * 4
            } else {
                pos += 1;
                (first as usize) * 4
            };

            if msg_len == 0 || pos + msg_len > plain.len() {
                break;
            }

            pos += msg_len;
            boundaries.push(pos);
        }

        if boundaries.len() <= 1 {
            return vec![chunk.to_vec()];
        }

        let mut parts = Vec::with_capacity(boundaries.len());
        let mut prev = 0usize;
        for end in boundaries {
            parts.push(chunk[prev..end].to_vec());
            prev = end;
        }
        if prev < chunk.len() {
            parts.push(chunk[prev..].to_vec());
        }
        parts
    }
}

pub fn split_mtproto_messages(chunk: &[u8]) -> Vec<Vec<u8>> {
    let mut pos = 0usize;
    let mut parts = Vec::new();
    while pos < chunk.len() {
        let len = chunk[pos] as usize;
        if len == 0 {
            break;
        }
        let total_len = if len == 0x7f {
            if pos + 4 > chunk.len() {
                break;
            }
            let raw_len = u32::from_le_bytes([chunk[pos + 1], chunk[pos + 2], chunk[pos + 3], 0]);
            4 + raw_len as usize
        } else {
            1 + (len as usize) * 4
        };

        let end = pos + total_len;
        if end > chunk.len() {
            break;
        }
        parts.push(chunk[pos..end].to_vec());
        pos += total_len;
    }
    if parts.is_empty() {
        vec![chunk.to_vec()]
    } else {
        parts
    }
}

fn ip_to_u32(ip: &str) -> u32 {
    let parsed = Ipv4Addr::from_str(ip).unwrap();
    u32::from(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_ranges_work() {
        assert!(is_telegram_ip("91.108.0.1"));
        assert!(!is_telegram_ip("1.1.1.1"));
    }

    #[test]
    fn dc_lookup_works() {
        let got = ip_to_dc("149.154.167.220").expect("must exist");
        assert_eq!(got.dc, 2);
    }

    #[test]
    fn extended_dc_lookup_works() {
        let got = ip_to_dc("149.154.166.120").expect("must exist");
        assert_eq!(got.dc, 4);
        assert!(got.is_media);
    }

    #[test]
    fn media_dc_lookup_works_for_167_35() {
        let got = ip_to_dc("149.154.167.35").expect("must exist");
        assert_eq!(got.dc, 2);
        assert!(got.is_media);
    }

    #[test]
    fn mtproto_split_small_batch() {
        let data = vec![
            0x04, 1, 2, 3, 4, // len=4 => 16 bytes expected, but data shorter
            0x00, 0x03, 9, 9, 9, 0x7f, 2, 0, 0, 9, 9,
        ];
        let parts = split_mtproto_messages(&data);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], data);
    }

    #[test]
    fn stateful_splitter_splits_two_messages() {
        let mut init = [0u8; 64];
        init[8..40].copy_from_slice(&[1u8; 32]);
        init[40..56].copy_from_slice(&[2u8; 16]);

        let mut enc = Aes256Ctr::new_from_slices(&init[8..40], &init[40..56]).unwrap();
        let mut skip = [0u8; 64];
        enc.apply_keystream(&mut skip);

        let plain = vec![
            0x01, 1, 2, 3, 4, // 1 * 4 bytes
            0x01, 5, 6, 7, 8,
        ];
        let mut chunk = plain.clone();
        enc.apply_keystream(&mut chunk);

        let mut splitter = MtProtoMessageSplitter::new(&init).unwrap();
        let parts = splitter.split(&chunk);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 5);
        assert_eq!(parts[1].len(), 5);
    }
}
