use common_protocol::PROVIDER_NAME;

pub const WINDOWS_PROVIDER_NAME: &str = PROVIDER_NAME;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_uses_project_name() {
        assert_eq!(WINDOWS_PROVIDER_NAME, "WinFaceUnlockProvider");
    }
}
