use zeroize::Zeroize;

#[derive(Debug, Eq, PartialEq)]
pub struct CredentialSecret {
    bytes: Vec<u8>,
}

impl CredentialSecret {
    pub fn from_utf8_password(password: impl Into<String>) -> Self {
        Self {
            bytes: password.into().into_bytes(),
        }
    }

    pub fn from_secret_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn expose_for_encryption(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for CredentialSecret {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_secret_keeps_bytes_until_drop() {
        let secret = CredentialSecret::from_utf8_password("password");

        assert_eq!(secret.expose_for_encryption(), b"password");
    }
}
