//! The per-account biometric profile: a stable pseudo-random byte vector derived from the account name, so an
//! account always plays the same way (mouse feel, sleep hour, ...). The scripts' lib reads it through `Host`.

const M: u32 = 0x5bd1_e995;

/// Number of 32 bit hash blocks in a profile.
pub const BLOCKS: u32 = 6;
/// Bytes in a profile, 4 per block.
pub const PROFILE_LEN: usize = BLOCKS as usize * 4;

fn mix(h: u32, k: u32) -> u32 {
    let k = k.wrapping_mul(M);
    let k = (k ^ (k >> 24)).wrapping_mul(M);
    h.wrapping_mul(M) ^ k
}

/// MurmurHash2A, little endian, as the lib has always used it.
pub fn murmur2a(text: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    let (chunks, rest) = text.as_chunks::<4>();
    for chunk in chunks {
        h = mix(h, u32::from_le_bytes(*chunk));
    }
    let mut tail = 0u32;
    for (i, byte) in rest.iter().enumerate() {
        tail |= (*byte as u32) << (8 * i);
    }
    h = mix(h, tail);
    h = mix(h, text.len() as u32);
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

/// The profile of an account, each block hashed with its index as seed.
pub fn profile(name: &[u8]) -> [u8; PROFILE_LEN] {
    let mut bytes = [0u8; PROFILE_LEN];
    for block in 0..BLOCKS {
        let h = murmur2a(name, block);
        bytes[block as usize * 4..][..4].copy_from_slice(&h.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values come from the Luau implementation this replaced.
    #[test]
    fn hashes_match_the_old_luau() {
        assert_eq!(murmur2a(b"player", 0), 3594132001);
        assert_eq!(murmur2a(b"abcd", 7), 139965599);
        assert_eq!(murmur2a(b"", 0), 0);
        assert_eq!(murmur2a(b"abcdefghijklmnopqrs", 0), 1770672524);
    }

    #[test]
    fn profiles_match_the_old_luau() {
        assert_eq!(
            profile(b"player"),
            [33, 26, 58, 214, 61, 74, 252, 228, 61, 146, 92, 98, 104, 229, 49, 209, 171, 34, 2, 231, 38, 6, 114, 226]
        );
        assert_eq!(
            profile(b"zezima"),
            [110, 94, 18, 242, 34, 167, 9, 136, 61, 72, 114, 50, 236, 89, 240, 188, 172, 106, 112, 212, 34, 143, 216, 251]
        );
    }
}
