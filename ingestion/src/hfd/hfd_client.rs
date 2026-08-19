use std::io::Read;

use crate::error::AppError;
use crate::secrets;

const LOGIN_URL: &str = "https://www.humanfertility.org/Account/Login";
const COHORT_TFR_ARCHIVE_URL: &str = "https://www.humanfertility.org/File/Download/Files/zip/tfr.zip";

const USERNAME_VARIABLE: &str = "HFD_USERNAME";
const PASSWORD_SECRET_NAME: &str = "hfd.siegelzc.password";

const ANTIFORGERY_FIELD: &str = "__RequestVerificationToken";
const COHORT_MEMBER_SUFFIX: &str = "tfrVH.txt";
const ZIP_MAGIC: [u8; 2] = [b'P', b'K'];

/// One country's cohort fertility file, as bytes, named by the archive member it came from.
#[derive(Debug, Clone)]
pub struct CohortFertilityFile {
    pub member_name: String,
    pub contents: String,
}

/// HFD serves downloads only to a logged-in account that has accepted the user agreement, and answers an
/// unauthenticated request with the registration page under a 200, so the archive is identified by its
/// magic bytes rather than by status.
pub async fn fetch_upstream() -> Result<Vec<CohortFertilityFile>, AppError> {
    let client: reqwest::Client = create_client()?;

    sign_in(&client).await?;

    let archive: Vec<u8> = download_cohort_archive(&client).await?;

    read_cohort_members(&archive)
}

/// Its own client rather than the shared one: the cookie jar holds a session for this source only.
fn create_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(concat!("eafora/", env!("CARGO_PKG_VERSION")))
        .cookie_store(true)
        .build()
        .map_err(|error| AppError::from(format!("could not build the hfd client; [error={error}]")))
}

async fn sign_in(client: &reqwest::Client) -> Result<(), AppError> {
    let form_html: String = client
        .get(LOGIN_URL)
        .send()
        .await
        .map_err(|error| AppError::from(format!("could not reach the hfd login form; [error={error}]")))?
        .text()
        .await
        .map_err(|error| AppError::from(format!("could not read the hfd login form; [error={error}]")))?;

    let token: String = read_antiforgery_token(&form_html)?;
    let username: String = dotenvy::var(USERNAME_VARIABLE)
        .map_err(|error| AppError::from(format!("{USERNAME_VARIABLE} is not set; [error={error}]")))?;
    let password: String = secrets::master_decrypt_utf8(PASSWORD_SECRET_NAME)?;

    let response: reqwest::Response = client
        .post(LOGIN_URL)
        .form(&[
            ("Email", username.as_str()),
            ("Password", password.as_str()),
            ("ReturnUrl", ""),
            (ANTIFORGERY_FIELD, token.as_str()),
        ])
        .send()
        .await
        .map_err(|error| AppError::from(format!("the hfd login request failed; [error={error}]")))?;

    let status: reqwest::StatusCode = response.status();

    if !status.is_success() {
        return Err(AppError::from(format!("hfd rejected the login; [status={status}]")));
    }

    Ok(())
}

/// The token is paired with a cookie the same response set, so it cannot be reused across sessions.
fn read_antiforgery_token(form_html: &str) -> Result<String, AppError> {
    let field_marker: String = format!("name=\"{ANTIFORGERY_FIELD}\"");
    let field_start: usize = form_html
        .find(&field_marker)
        .ok_or_else(|| AppError::from(format!("the hfd login form carries no {ANTIFORGERY_FIELD}")))?;
    let value_marker: &str = "value=\"";
    let value_start: usize = form_html[field_start..]
        .find(value_marker)
        .map(|offset| field_start + offset + value_marker.len())
        .ok_or_else(|| AppError::from(format!("the hfd {ANTIFORGERY_FIELD} field carries no value")))?;
    let value_length: usize = form_html[value_start..]
        .find('"')
        .ok_or_else(|| AppError::from(format!("the hfd {ANTIFORGERY_FIELD} value is unterminated")))?;

    Ok(form_html[value_start..value_start + value_length].to_string())
}

async fn download_cohort_archive(client: &reqwest::Client) -> Result<Vec<u8>, AppError> {
    let bytes: Vec<u8> = client
        .get(COHORT_TFR_ARCHIVE_URL)
        .send()
        .await
        .map_err(|error| AppError::from(format!("the hfd archive request failed; [error={error}]")))?
        .bytes()
        .await
        .map_err(|error| AppError::from(format!("could not read the hfd archive; [error={error}]")))?
        .to_vec();

    if !bytes.starts_with(&ZIP_MAGIC) {
        return Err(AppError::from(format!(
            "hfd served a page rather than the archive, which means the account is not signed in or has not accepted the user agreement; [bytes={}]",
            bytes.len(),
        )));
    }

    Ok(bytes)
}

/// Reads only the cohort members. HFD's input files carry each provider's own licence and are excluded
/// from this crate entirely; the by-birth-order companion is a different statistic.
pub fn read_cohort_members(archive: &[u8]) -> Result<Vec<CohortFertilityFile>, AppError> {
    let reader = std::io::Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|error| AppError::from(format!("could not open the hfd archive; [error={error}]")))?;

    let member_names: Vec<String> = (0..zip.len())
        .filter_map(|index| zip.by_index(index).ok().map(|member| member.name().to_string()))
        .filter(|name| name.ends_with(COHORT_MEMBER_SUFFIX))
        .collect();

    let mut files: Vec<CohortFertilityFile> = Vec::with_capacity(member_names.len());

    for member_name in member_names {
        let mut member = zip
            .by_name(&member_name)
            .map_err(|error| AppError::from(format!("could not read {member_name}; [error={error}]")))?;
        let mut contents: String = String::new();
        member
            .read_to_string(&mut contents)
            .map_err(|error| AppError::from(format!("{member_name} is not text; [error={error}]")))?;

        files.push(CohortFertilityFile { member_name, contents });
    }

    Ok(files)
}
