use crate::data::{Account, Session};

#[derive(Default)]
pub(super) struct AccountFilter {
    pub query: String,
    pub running_only: bool,
}

impl AccountFilter {
    pub fn matches(&self, account: &Account, email: Option<&str>, query: &str) -> bool {
        (!self.running_only || account.running_pid.is_some())
            && (query.is_empty()
                || contains_words(
                    &format!(
                        "{}\n{}\n{}",
                        account.name,
                        account.id,
                        email.unwrap_or_default()
                    )
                    .to_lowercase(),
                    query,
                ))
    }
}

// Each word may match a different field. A newline prevents accidental matches
// across the boundary between the title and folder (or a label and email).
fn contains_words(fields: &str, lower_query: &str) -> bool {
    lower_query
        .split_whitespace()
        .all(|word| fields.contains(word))
}

pub(super) struct SessionFilter {
    pub query: String,
    keys: Vec<String>,
    pub visible: Vec<usize>,
}

impl SessionFilter {
    pub fn new(sessions: &[Session]) -> Self {
        Self {
            query: String::new(),
            keys: sessions
                .iter()
                .map(|s| format!("{}\n{}", s.title, s.folder).to_lowercase())
                .collect(),
            visible: (0..sessions.len()).collect(),
        }
    }

    pub fn refresh(&mut self) {
        let query = self.query.to_lowercase();
        self.visible.clear();
        self.visible.extend(
            self.keys
                .iter()
                .enumerate()
                .filter(|(_, fields)| contains_words(fields, &query))
                .map(|(i, _)| i),
        );
    }

    pub fn checked_count(&self, checked: &[bool]) -> usize {
        self.visible.iter().filter(|&&i| checked[i]).count()
    }

    pub fn toggle_visible(&self, checked: &mut [bool]) {
        let select = self.checked_count(checked) < self.visible.len();
        for &i in &self.visible {
            checked[i] = select;
        }
    }
}
