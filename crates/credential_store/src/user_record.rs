use common_protocol::{AccountType, CredentialRef, UserId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserRecord {
    pub user_id: UserId,
    pub user_sid: String,
    pub username: String,
    pub account_type: AccountType,
    pub credential_ref: CredentialRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_record_keeps_credential_as_reference() {
        let record = UserRecord {
            user_id: UserId("user-1".to_owned()),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "Liu".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef("cred-1".to_owned()),
        };

        assert_eq!(record.credential_ref, CredentialRef("cred-1".to_owned()));
    }
}
