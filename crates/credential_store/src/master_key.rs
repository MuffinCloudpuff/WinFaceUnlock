use zeroize::Zeroize;

pub const MASTER_KEY_LEN: usize = 32;

#[derive(Debug, Eq, PartialEq)]
pub struct MasterKey {
    bytes: [u8; MASTER_KEY_LEN],
}

impl MasterKey {
    pub fn expose_for_cryptographic_use(&self) -> &[u8; MASTER_KEY_LEN] {
        &self.bytes
    }

    pub(crate) fn from_bytes(bytes: [u8; MASTER_KEY_LEN]) -> Self {
        Self { bytes }
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_keeps_fixed_length() {
        let key = MasterKey::from_bytes([1_u8; MASTER_KEY_LEN]);

        assert_eq!(key.expose_for_cryptographic_use().len(), MASTER_KEY_LEN);
    }
}
