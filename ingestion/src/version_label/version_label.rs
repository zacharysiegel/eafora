use std::sync::LazyLock;

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::artifact::artifact_db;
use crate::error::AppError;

const SURNAMES_RAW: &str = include_str!("./nobel_surnames.txt");

static SURNAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    SURNAMES_RAW
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
});

const MAX_GENERATION_ATTEMPTS: usize = 16;

pub async fn generate(pool: &PgPool) -> Result<String, AppError> {
    let date_prefix: String = Utc::now().format("%Y-%m-%d").to_string();

    for _ in 0..MAX_GENERATION_ATTEMPTS {
        let surname: &'static str = pick_random_surname();
        let candidate: String = format!("{}-{}", date_prefix, surname);

        let exists: bool = artifact_db::read_artifact_version_exists(pool, &candidate).await?;
        if !exists {
            return Ok(candidate);
        }
    }

    Err(AppError::from(format!(
        "could not generate a unique version label after {} attempts on {}",
        MAX_GENERATION_ATTEMPTS, date_prefix,
    )))
}

fn pick_random_surname() -> &'static str {
    let uuid: Uuid = Uuid::now_v7();
    let bytes: [u8; 16] = uuid.into_bytes();
    let index: usize = u16::from_be_bytes([bytes[14], bytes[15]]) as usize % SURNAMES.len();
    SURNAMES[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surnames_list_parses_with_at_least_100_entries() {
        assert!(SURNAMES.len() >= 100, "expected at least 100 surnames, got {}", SURNAMES.len());
    }

    #[test]
    fn pick_random_surname_returns_an_entry_from_the_list() {
        let picked: &'static str = pick_random_surname();
        assert!(SURNAMES.contains(&picked));
    }
}
