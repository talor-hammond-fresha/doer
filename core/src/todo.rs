use serde::{Deserialize, Serialize};

use crate::id::TodoId;

pub const SECONDS_PER_DAY: i64 = 86_400;

/// Field order is the on-disk key order and matches what the Elixir build wrote.
/// Reordering these fields is a data-format break.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: TodoId,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub completed_at: Option<i64>,
    /// Flattened with every field skipped when empty, so a plain todo stays byte-identical
    /// to what earlier builds wrote, and they carry these keys through as unknown fields.
    #[serde(flatten)]
    pub meta: Meta,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub priority: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub due: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub remind_at: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_step: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub links: Vec<Link>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub session: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub context: Vec<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Now,
    Soon,
    Someday,
}

impl Priority {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "now" | "high" | "urgent" | "🔥" | "🔥 now" => Some(Self::Now),
            "soon" | "medium" | "normal" => Some(Self::Soon),
            "someday" | "low" | "later" => Some(Self::Someday),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::Soon => "soon",
            Self::Someday => "someday",
        }
    }

    #[must_use]
    pub fn cycle(current: Option<Self>) -> Option<Self> {
        match current {
            None => Some(Self::Now),
            Some(Self::Now) => Some(Self::Soon),
            Some(Self::Soon) => Some(Self::Someday),
            Some(Self::Someday) => None,
        }
    }
}

impl Meta {
    #[must_use]
    pub fn priority(&self) -> Option<Priority> {
        self.priority.as_deref().and_then(Priority::parse)
    }

    #[must_use]
    pub fn due_day(&self) -> Option<i64> {
        self.due.as_deref().and_then(parse_ymd)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[must_use]
pub fn parse_ymd(raw: &str) -> Option<i64> {
    let mut parts = raw.trim().splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.get(..2)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[must_use]
pub fn ymd_of(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

impl Todo {
    #[must_use]
    pub fn new(text: impl Into<String>, now: i64) -> Self {
        Self {
            id: TodoId::generate(),
            text: text.into(),
            done: false,
            created_at: now,
            completed_at: None,
            meta: Meta::default(),
        }
    }

    pub fn toggle(&mut self, now: i64) {
        if self.done {
            self.done = false;
            self.completed_at = None;
        } else {
            self.done = true;
            self.completed_at = Some(now);
        }
    }

    /// Whole days since creation, floored, never negative.
    #[must_use]
    pub fn age_days(&self, now: i64) -> i64 {
        days_between(self.created_at, now)
    }

    /// Whole days since completion, or `None` for an active todo.
    #[must_use]
    pub fn completed_days(&self, now: i64) -> Option<i64> {
        self.completed_at.map(|at| days_between(at, now))
    }
}

fn days_between(from: i64, to: i64) -> i64 {
    to.saturating_sub(from).max(0) / SECONDS_PER_DAY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_epoch_days() {
        assert_eq!(parse_ymd("1970-01-01"), Some(0));
        assert_eq!(parse_ymd("2026-11-10"), Some(20_767));
        assert_eq!(ymd_of(20_767), (2026, 11, 10));
        assert_eq!(ymd_of(parse_ymd("2024-02-29").unwrap()), (2024, 2, 29));
        assert_eq!(parse_ymd("nope"), None);
    }

    #[test]
    fn meta_is_omitted_when_empty_and_kept_in_order_when_set() {
        let mut todo = Todo::new("x", 0);
        assert!(!serde_json::to_string(&todo).unwrap().contains("priority"));
        todo.meta.priority = Some("now".into());
        todo.meta.links.push(Link {
            label: "PR".into(),
            url: "https://x".into(),
        });
        let json = serde_json::to_string(&todo).unwrap();
        assert!(json.ends_with(r#""completed_at":null,"priority":"now","links":[{"label":"PR","url":"https://x"}]}"#), "{json}");
        let back: Todo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, todo);
    }

    #[test]
    fn a_wrongly_shaped_field_reads_as_unset_rather_than_failing() {
        let todo: Todo = serde_json::from_str(r#"{"id":"a","priority":"p0"}"#).unwrap();
        assert_eq!(todo.meta.priority(), None);
        let todo: Todo = serde_json::from_str(r#"{"id":"a","priority":1,"links":"x"}"#).unwrap();
        assert_eq!(todo.meta, Meta::default());
    }

    const T0: i64 = 1_700_000_000;

    #[test]
    fn toggle_sets_and_clears_completed_at() {
        let mut todo = Todo::new("write tests", T0);
        assert!(!todo.done);
        assert_eq!(todo.completed_at, None);

        todo.toggle(T0 + 10);
        assert!(todo.done);
        assert_eq!(todo.completed_at, Some(T0 + 10));

        todo.toggle(T0 + 20);
        assert!(!todo.done);
        assert_eq!(todo.completed_at, None);
    }

    #[test]
    fn age_days_floors_at_day_boundaries() {
        let todo = Todo::new("x", T0);
        assert_eq!(todo.age_days(T0), 0);
        assert_eq!(todo.age_days(T0 + SECONDS_PER_DAY - 1), 0);
        assert_eq!(todo.age_days(T0 + SECONDS_PER_DAY), 1);
        assert_eq!(todo.age_days(T0 + 2 * SECONDS_PER_DAY + 5), 2);
    }

    #[test]
    fn a_clock_that_went_backwards_reads_as_zero_days() {
        let todo = Todo::new("x", T0);
        assert_eq!(todo.age_days(T0 - SECONDS_PER_DAY), 0);
    }

    #[test]
    fn completed_days_is_none_while_active() {
        let mut todo = Todo::new("x", T0);
        assert_eq!(todo.completed_days(T0), None);
        todo.toggle(T0);
        assert_eq!(todo.completed_days(T0 + 3 * SECONDS_PER_DAY), Some(3));
    }

    #[test]
    fn json_key_order_and_shape_match_the_elixir_format() {
        let todo = Todo {
            id: TodoId::from("8257339108cf0e12"),
            text: "buy milk".into(),
            done: false,
            created_at: 1_755_000_000,
            completed_at: None,
            meta: Meta::default(),
        };
        let json = serde_json::to_string_pretty(&todo).expect("serialize");
        assert_eq!(
            json,
            "{\n  \"id\": \"8257339108cf0e12\",\n  \"text\": \"buy milk\",\n  \"done\": false,\n  \"created_at\": 1755000000,\n  \"completed_at\": null\n}"
        );
    }

    #[test]
    fn missing_fields_default_rather_than_failing_the_file() {
        let todo: Todo = serde_json::from_str(r#"{"id":"abc"}"#).expect("lenient decode");
        assert_eq!(todo.text, "");
        assert!(!todo.done);
        assert_eq!(todo.created_at, 0);
        assert_eq!(todo.completed_at, None);
    }
}
