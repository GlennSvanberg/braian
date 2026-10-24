use uuid::{Builder, Uuid, Variant, Version};

/// Deterministic UUIDv7 stream.
///
/// The timestamp is fixed. The sequence occupies the low 32 bits, so ids from
/// one stream are unique. The seed changes the remaining random bits.
pub struct Ids {
    seed: u64,
    seq: u64,
}

impl Ids {
    pub fn new(seed: u64) -> Self {
        Self { seed, seq: 0 }
    }

    pub fn next(&mut self) -> Uuid {
        self.seq += 1;
        let timestamp_ms: u64 = 1_700_000_000_000;
        let mut bytes = [0u8; 16];
        bytes[0..6].copy_from_slice(&timestamp_ms.to_be_bytes()[2..8]);
        bytes[6..8].copy_from_slice(&(self.seed as u16).to_be_bytes());
        let mixed = (self.seed << 32) | (self.seq & 0xFFFF_FFFF);
        bytes[8..16].copy_from_slice(&mixed.to_be_bytes());
        let mut builder = Builder::from_bytes(bytes);
        builder
            .set_version(Version::SortRand)
            .set_variant(Variant::RFC4122);
        *builder.as_uuid()
    }
}
