#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareFingerprint {
    pub machine_sid_hash: Option<String>,
    pub firmware_uuid_hash: Option<String>,
    pub system_drive_hash: Option<String>,
}

impl HardwareFingerprint {
    pub fn empty() -> Self {
        Self {
            machine_sid_hash: None,
            firmware_uuid_hash: None,
            system_drive_hash: None,
        }
    }

    pub fn has_any_signal(&self) -> bool {
        self.machine_sid_hash.is_some()
            || self.firmware_uuid_hash.is_some()
            || self.system_drive_hash.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HardwareBindingError {
    CollectionUnavailable,
    FingerprintMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fingerprint_has_no_signal() {
        assert!(!HardwareFingerprint::empty().has_any_signal());
    }
}
