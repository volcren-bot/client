use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Swap {
    pub target: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EntrySwaps {
    pub swaps: Vec<Swap>,
    pub alt: Vec<Swap>,
}

pub fn parse(text: &str) -> Vec<Swap> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.trim().split(',');
            let target = parts.next()?.trim();
            let options: Vec<String> = parts
                .map(str::trim)
                .filter(|option| !option.is_empty())
                .map(str::to_owned)
                .collect();
            (!target.is_empty() && !options.is_empty()).then(|| Swap {
                target: target.to_owned(),
                options,
            })
        })
        .collect()
}

pub fn user_folder(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.trim_start_matches("\\\\?\\").split('\\').collect();
    let users = (2..segments.len().saturating_sub(1)).find(|&i| {
        let id = segments[i + 1];
        segments[i].eq_ignore_ascii_case("users")
            && !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_digit())
    })?;
    Some(segments[..users + 2].join("\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_targets_with_their_options_in_order() {
        let swaps = parse("1\nMan,Attack,Pickpocket\nOrdan,Trade\nCoal,Buy 50,Buy 10\n");
        assert_eq!(swaps.len(), 3);
        assert_eq!(swaps[0].target, "Man");
        assert_eq!(swaps[0].options, ["Attack", "Pickpocket"]);
        assert_eq!(swaps[2].options, ["Buy 50", "Buy 10"]);
    }

    #[test]
    fn it_reads_an_empty_file() {
        assert!(parse("1\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn it_skips_lines_with_no_option() {
        assert!(parse("1\nOrdan\n,Trade\n").is_empty());
    }

    #[test]
    fn it_finds_the_account_folder_of_a_handle_path() {
        let path = "\\\\?\\C:\\users\\a\\AppData\\Local\\Studio\\Game\\users\\750210\\Tracker.db";
        assert_eq!(
            user_folder(path).as_deref(),
            Some("C:\\users\\a\\AppData\\Local\\Studio\\Game\\users\\750210")
        );
    }

    #[test]
    fn it_finds_the_account_folder_itself() {
        assert_eq!(
            user_folder("C:\\Game\\users\\750210").as_deref(),
            Some("C:\\Game\\users\\750210")
        );
    }

    #[test]
    fn it_skips_a_numeric_windows_profile() {
        assert_eq!(
            user_folder("C:\\users\\1001\\AppData\\Local\\Game\\users\\750210\\x").as_deref(),
            Some("C:\\users\\1001\\AppData\\Local\\Game\\users\\750210")
        );
        assert_eq!(user_folder("C:\\users\\1001\\AppData\\x"), None);
    }

    #[test]
    fn it_ignores_other_paths() {
        assert_eq!(user_folder("C:\\Game\\data\\x.dat"), None);
        assert_eq!(user_folder("C:\\Game\\users\\Highlights\\x"), None);
    }
}
