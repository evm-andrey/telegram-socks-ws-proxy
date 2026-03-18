use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

pub struct MtProtoMessageSplitter {
    cipher: Aes256Ctr,
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

fn split_plain_mtproto_messages(chunk: &[u8]) -> Vec<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::{split_plain_mtproto_messages, MtProtoMessageSplitter};
    use aes::cipher::{KeyIvInit, StreamCipher};
    use aes::Aes256;

    type Aes256Ctr = ctr::Ctr128BE<Aes256>;

    #[test]
    fn mtproto_split_small_batch() {
        let data = vec![0x04, 1, 2, 3, 4, 0x00, 0x03, 9, 9, 9, 0x7f, 2, 0, 0, 9, 9];
        let parts = split_plain_mtproto_messages(&data);
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

        let plain = vec![0x01, 1, 2, 3, 4, 0x01, 5, 6, 7, 8];
        let mut chunk = plain.clone();
        enc.apply_keystream(&mut chunk);

        let mut splitter = MtProtoMessageSplitter::new(&init).unwrap();
        let parts = splitter.split(&chunk);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 5);
        assert_eq!(parts[1].len(), 5);
    }
}
