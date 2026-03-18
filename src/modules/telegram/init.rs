use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramInitInfo {
    pub dc: Option<u8>,
    pub is_media: bool,
}

pub fn extract_dc(data: &[u8]) -> TelegramInitInfo {
    if data.len() < 64 {
        return TelegramInitInfo {
            dc: None,
            is_media: false,
        };
    }

    let plain = decrypt_header(data).unwrap_or([0u8; 8]);
    let proto = u32::from_le_bytes(plain[0..4].try_into().unwrap_or([0u8; 4]));
    if !matches!(proto, 0xEFEFEFEF | 0xEEEEEEEE | 0xDDDDDDDD) {
        return TelegramInitInfo {
            dc: None,
            is_media: false,
        };
    }

    let dc_raw = i16::from_le_bytes(plain[4..6].try_into().unwrap_or([0u8; 2]));
    let dc = dc_raw.unsigned_abs() as u8;
    if (1..=5).contains(&dc) {
        TelegramInitInfo {
            dc: Some(dc),
            is_media: dc_raw < 0,
        }
    } else {
        TelegramInitInfo {
            dc: None,
            is_media: false,
        }
    }
}

pub fn patch_init_dc(data: &[u8], dc: i16) -> Vec<u8> {
    if data.len() < 64 {
        return data.to_vec();
    }

    let stream = match keystream(data) {
        Some(stream) => stream,
        None => return data.to_vec(),
    };
    let mut out = data[..64].to_vec();
    let dc_bytes = dc.to_le_bytes();
    out[60] = stream[60] ^ dc_bytes[0];
    out[61] = stream[61] ^ dc_bytes[1];
    if data.len() > 64 {
        out.extend_from_slice(&data[64..]);
    }
    out
}

fn decrypt_header(data: &[u8]) -> Option<[u8; 8]> {
    let stream = keystream(data)?;
    let mut plain = [0u8; 8];
    for idx in 0..8 {
        plain[idx] = data[56 + idx] ^ stream[56 + idx];
    }
    Some(plain)
}

fn keystream(data: &[u8]) -> Option<[u8; 64]> {
    let key = &data[8..40];
    let iv = &data[40..56];
    let mut cipher = Aes256Ctr::new_from_slices(key, iv).ok()?;
    let mut stream = [0u8; 64];
    cipher.apply_keystream(&mut stream);
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::{extract_dc, patch_init_dc};
    use aes::cipher::{KeyIvInit, StreamCipher};
    use aes::Aes256;

    type Aes256Ctr = ctr::Ctr128BE<Aes256>;

    fn build_init(dc: i16) -> Vec<u8> {
        let mut init = [0u8; 64];
        init[8..40].copy_from_slice(&[7u8; 32]);
        init[40..56].copy_from_slice(&[9u8; 16]);

        let mut cipher = Aes256Ctr::new_from_slices(&init[8..40], &init[40..56]).unwrap();
        let mut stream = [0u8; 64];
        cipher.apply_keystream(&mut stream);

        let mut plain = [0u8; 8];
        plain[0..4].copy_from_slice(&0xEFEFEFEFu32.to_le_bytes());
        plain[4..6].copy_from_slice(&dc.to_le_bytes());
        for idx in 0..8 {
            init[56 + idx] = stream[56 + idx] ^ plain[idx];
        }

        init.to_vec()
    }

    #[test]
    fn extract_dc_reads_non_media_and_media() {
        let non_media = extract_dc(&build_init(3));
        assert_eq!(non_media.dc, Some(3));
        assert!(!non_media.is_media);

        let media = extract_dc(&build_init(-4));
        assert_eq!(media.dc, Some(4));
        assert!(media.is_media);
    }

    #[test]
    fn patch_init_dc_updates_encoded_dc() {
        let init = build_init(0);

        let patched = patch_init_dc(&init, 2);
        assert_eq!(extract_dc(&patched).dc, Some(2));
        assert!(!extract_dc(&patched).is_media);

        let patched_media = patch_init_dc(&init, -5);
        assert_eq!(extract_dc(&patched_media).dc, Some(5));
        assert!(extract_dc(&patched_media).is_media);
    }

    #[test]
    fn short_buffers_are_left_unchanged() {
        let data = vec![1u8, 2, 3];
        assert_eq!(patch_init_dc(&data, 2), data);
        assert_eq!(
            extract_dc(&data),
            super::TelegramInitInfo {
                dc: None,
                is_media: false,
            }
        );
    }
}
