#[derive(Debug)]
pub struct SubscriberEmail(String);

impl SubscriberEmail {
    pub fn parse(s: String) -> Result<SubscriberEmail, String> {
        if !validator::ValidateEmail::validate_email(&s) {
            Err(format!("{} is not a valid subscriber email.", s))
        } else {
            Ok(Self(s))
        }
    }
}

impl AsRef<String> for SubscriberEmail {
    fn as_ref(&self) -> &String {
        &self.0
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use claim::assert_err;
    use fake::{Fake, faker::internet::en::SafeEmail};

    #[test]
    fn empty_email_is_rejected() {
        let email = "".to_string();
        assert_err!(SubscriberEmail::parse(email));
    }

    #[test]
    fn email_missing_at_symbol_is_rejected() {
        let email = "ursuladomain.com".to_string();
        assert_err!(SubscriberEmail::parse(email));
    }

    #[test]
    fn email_missing_subject_is_rejected() {
        let email = "@domain.com".to_string();
        assert_err!(SubscriberEmail::parse(email));
    }

    #[test]
    fn valid_email_are_parsed_successfully() {
        let email = SafeEmail().fake();
        claim::assert_ok!(SubscriberEmail::parse(email));
    }
}
