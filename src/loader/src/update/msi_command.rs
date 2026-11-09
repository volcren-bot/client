#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scope {
    PerUser,
    PerMachine,
}

impl Scope {
    pub fn word(self) -> &'static str {
        match self {
            Self::PerUser => "user",
            Self::PerMachine => "machine",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "user" => Some(Self::PerUser),
            "machine" => Some(Self::PerMachine),
            _ => None,
        }
    }
}

pub fn command_line(msi: &str, log: &str, folder: &str, scope: Scope) -> String {
    let all_users = match scope {
        Scope::PerUser => r#"ALLUSERS="""#,
        Scope::PerMachine => "ALLUSERS=1",
    };
    let folder = folder.trim_end_matches('\\');
    format!(
        r#"/i "{msi}" /passive /norestart /l*v "{log}" {all_users} APPLICATIONFOLDER="{folder}""#
    )
}

pub fn failure(code: Option<i32>, log: &str) -> Option<String> {
    match code {
        Some(0) => None,
        Some(1602) => Some("The update wasn't installed because an administrator didn't approve it.".into()),
        Some(1641 | 3010) => Some("Restart Windows to finish the update.".into()),
        Some(1618) => {
            Some("Another installation was running, so the update didn't install. Try again when it's done.".into())
        }
        Some(code) => Some(format!(
            "The update didn't install (code {code}), the old version stays. The details are in {log}."
        )),
        None => Some(format!(
            "The update didn't install, the old version stays. The details are in {log}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_per_user_install_where_it_is() {
        assert_eq!(
            command_line(
                r"C:\d\a.msi",
                r"C:\d\a.log",
                r"C:\Program Files\X\",
                Scope::PerUser
            ),
            r#"/i "C:\d\a.msi" /passive /norestart /l*v "C:\d\a.log" ALLUSERS="" APPLICATIONFOLDER="C:\Program Files\X""#
        );
    }

    #[test]
    fn a_per_machine_install_stays_per_machine() {
        assert!(
            command_line("a.msi", "a.log", r"C:\X", Scope::PerMachine)
                .contains(r#"ALLUSERS=1 APPLICATIONFOLDER="C:\X""#)
        );
    }

    #[test]
    fn the_scope_survives_its_argument() {
        for scope in [Scope::PerUser, Scope::PerMachine] {
            assert_eq!(Scope::parse(scope.word()), Some(scope));
        }
        assert_eq!(Scope::parse("other"), None);
    }

    #[test]
    fn success_says_nothing() {
        assert_eq!(failure(Some(0), "l"), None);
    }

    #[test]
    fn a_declined_prompt_is_reported_so_it_doesnt_repeat_at_once() {
        assert!(failure(Some(1602), "l").unwrap().contains("administrator"));
    }

    #[test]
    fn a_needed_restart_is_not_called_a_failure() {
        assert!(
            failure(Some(3010), "l")
                .unwrap()
                .contains("Restart Windows")
        );
    }

    #[test]
    fn a_failure_points_at_the_log() {
        assert!(failure(Some(1603), "the.log").unwrap().contains("the.log"));
        assert!(failure(None, "the.log").unwrap().contains("the.log"));
    }
}
