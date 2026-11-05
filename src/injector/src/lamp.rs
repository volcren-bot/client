//! Asks the loader which skill an account spends its experience lamps on, chosen on its Accounts page.

use std::sync::{Mutex, PoisonError};

/// Asks the loader about an account hash: the skill, `None` if none is set, or why it can't say.
pub type Lookup = Box<dyn FnMut(&str) -> Result<Option<String>, String> + Send>;

static LOOKUP: Mutex<Option<Lookup>> = Mutex::new(None);

/// Where `skill_for` asks. Without one (a run with no loader) every answer is `None`.
pub fn set_lookup(lookup: Lookup) {
    *LOOKUP.lock().unwrap_or_else(PoisonError::into_inner) = Some(lookup);
}

/// The skill set for the account with this name, which the caller reads from the client itself.
pub fn skill_for(account: &str) -> Result<Option<String>, String> {
    let mut lookup = LOOKUP.lock().unwrap_or_else(PoisonError::into_inner);
    match lookup.as_mut() {
        Some(lookup) => lookup(&crate::bank_pin::hash_name(account)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_asks_by_the_hash_of_the_account_name() {
        assert_eq!(skill_for("anyone"), Ok(None), "no loader: nothing is set");
        let hash = crate::bank_pin::hash_name("some_name");
        set_lookup(Box::new(move |asked| {
            Ok((asked == hash).then(|| "Slayer".to_owned()))
        }));
        assert_eq!(skill_for("Some Name"), Ok(Some("Slayer".to_owned())));
        assert_eq!(skill_for("other"), Ok(None));
    }
}
