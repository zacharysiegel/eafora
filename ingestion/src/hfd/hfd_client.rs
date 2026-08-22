use std::io::Read;

use chrono::NaiveDate;

use crate::error::AppError;
use crate::hfd::hfd_model::{ParsedHfdPublication, ParsedHfdStatisticValue};
use crate::secrets;

const LOGIN_URL: &str = "https://www.humanfertility.org/Account/Login";
const COHORT_TFR_ARCHIVE_URL: &str = "https://www.humanfertility.org/File/Download/Files/zip/tfr.zip";

const USERNAME_VARIABLE: &str = "HFD_USERNAME";
const PASSWORD_SECRET_NAME: &str = "hfd.siegelzc.password";

const ANTIFORGERY_FIELD: &str = "__RequestVerificationToken";
const SESSION_COOKIE: &str = "Authorization";
const ZIP_MAGIC: [u8; 2] = [b'P', b'K'];

/// `VH` and `RR` are HFD's Lexis-shape suffixes: a cohort file is indexed by birth cohort, a period file by
/// calendar year.
pub const COHORT_MEMBER: &str = "tfrVH.txt";
pub const PERIOD_MEMBER: &str = "tfrRR.txt";

const LAST_MODIFIED_PREFIX: &str = "Last modified:";
const HEADER_LINE_INDEX: usize = 2;
const ABSENT_VALUE: &str = ".";

const COLUMN_CODE: &str = "Code";

/// The two files share a layout and differ only in what their period and value columns are called.
pub const COHORT_FERTILITY_COLUMNS: FertilityColumns = FertilityColumns { period: "Cohort", value: "CCF" };
pub const PERIOD_FERTILITY_COLUMNS: FertilityColumns = FertilityColumns { period: "Year", value: "TFR" };

/// The cookies an authenticated HFD request must carry, obtained by signing in.
pub struct HfdSession {
    cookie_header: String,
}

#[derive(Debug, Clone, Copy)]
pub struct FertilityColumns {
    pub period: &'static str,
    pub value: &'static str,
}

/// Resolved from the header once, so an upstream column addition or reordering cannot silently shift which
/// value is read.
struct FertilityColumnIndices {
    code_index: usize,
    period_index: usize,
    value_index: usize,
}

impl FertilityColumnIndices {
    fn from_header(header_line: &str, columns: FertilityColumns) -> Result<FertilityColumnIndices, AppError> {
        let names: Vec<&str> = header_line.split_whitespace().collect();

        Ok(FertilityColumnIndices {
            code_index: index_of_column(&names, COLUMN_CODE)?,
            period_index: index_of_column(&names, columns.period)?,
            value_index: index_of_column(&names, columns.value)?,
        })
    }

    fn read_row(&self, line: &str) -> Result<ParsedHfdStatisticValue, AppError> {
        let fields: Vec<&str> = line.split_whitespace().collect();

        let hfd_code: &str = self.field(&fields, self.code_index, line)?;
        let declared_period: &str = self.field(&fields, self.period_index, line)?;
        let declared_value: &str = self.field(&fields, self.value_index, line)?;

        let period_year: i32 = declared_period.parse::<i32>()
            .map_err(|error| AppError::from(format!(
                "the hfd period is unparsable; [period={declared_period} error={error}]",
            )))?;

        let value: Option<f64> = if declared_value == ABSENT_VALUE {
            None
        } else {
            let parsed: f64 = declared_value.parse::<f64>()
                .map_err(|error| AppError::from(format!(
                    "the hfd value is unparsable; [code={hfd_code} period={period_year} value={declared_value} error={error}]",
                )))?;

            Some(parsed)
        };

        Ok(ParsedHfdStatisticValue {
            hfd_code: hfd_code.to_string(),
            period_year,
            value,
        })
    }

    fn field<'a>(&self, fields: &[&'a str], index: usize, line: &str) -> Result<&'a str, AppError> {
        fields.get(index).copied().ok_or_else(|| AppError::from(format!(
            "an hfd row has fewer fields than the header declares; [fields={} wanted={} line={}]",
            fields.len(),
            index + 1,
            line.trim(),
        )))
    }
}


pub async fn fetch_upstream() -> Result<Vec<u8>, AppError> {
    let client: reqwest::Client = create_client()?;
    let session: HfdSession = sign_in(&client).await?;

    download_cohort_archive(&client, &session).await
}

/// Does not follow redirects: a successful login answers 302 and carries the session cookie on that response.
fn create_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(concat!("eafora/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AppError::from(format!("could not build the hfd client; [error={error}]")))
}

/// A failed login answers 200 by re-rendering the form, so a successful status does not mean success.
async fn sign_in(client: &reqwest::Client) -> Result<HfdSession, AppError> {
    let form_response: reqwest::Response = client
        .get(LOGIN_URL)
        .send()
        .await
        .map_err(|error| AppError::from(format!("could not reach the hfd login form; [error={error}]")))?;
    let antiforgery_cookies: String = read_cookie_header(&form_response);
    let form_html: String = form_response
        .text()
        .await
        .map_err(|error| AppError::from(format!("could not read the hfd login form; [error={error}]")))?;

    let token: String = read_antiforgery_token(&form_html)?;
    let username: String = dotenvy::var(USERNAME_VARIABLE)
        .map_err(|error| AppError::from(format!("{USERNAME_VARIABLE} is not set; [error={error}]")))?;
    let password: String = secrets::master_decrypt_utf8(PASSWORD_SECRET_NAME)?;

    let response: reqwest::Response = client
        .post(LOGIN_URL)
        .header(reqwest::header::COOKIE, &antiforgery_cookies)
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
    let cookie_header: String = read_cookie_header(&response);

    if !cookie_header.contains(SESSION_COOKIE) {
        return Err(AppError::from(format!(
            "hfd issued no {SESSION_COOKIE} cookie, so the credentials were rejected; [status={status}]",
        )));
    }

    Ok(HfdSession { cookie_header })
}

fn read_cookie_header(response: &reqwest::Response) -> String {
    let set_cookie_values = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok());

    create_cookie_header(set_cookie_values)
}

/// Keeps the `name=value` of each `Set-Cookie`, dropping the attributes that follow it.
fn create_cookie_header<'a>(set_cookie_values: impl Iterator<Item = &'a str>) -> String {
    set_cookie_values
        .filter_map(|value| value.split(';').next())
        .collect::<Vec<&str>>()
        .join("; ")
}

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

async fn download_cohort_archive(
    client: &reqwest::Client,
    session: &HfdSession,
) -> Result<Vec<u8>, AppError> {
    let bytes: Vec<u8> = client
        .get(COHORT_TFR_ARCHIVE_URL)
        .header(reqwest::header::COOKIE, &session.cookie_header)
        .send()
        .await
        .map_err(|error| AppError::from(format!("the hfd archive request failed; [error={error}]")))?
        .bytes()
        .await
        .map_err(|error| AppError::from(format!("could not read the hfd archive; [error={error}]")))?
        .to_vec();

    /* An account that has not accepted the user agreement is served the registration page under a 200, so
       the archive is identified by its magic bytes rather than by status. */
    if !bytes.starts_with(&ZIP_MAGIC) {
        return Err(AppError::from(format!(
            "hfd served a page rather than the archive, which means the account has not accepted the user agreement; [bytes={}]",
            bytes.len(),
        )));
    }

    Ok(bytes)
}

/// HFD's input files carry each provider's own licence and are excluded from this crate entirely; the
/// by-birth-order companions are a different statistic.
pub fn read_member(archive: &[u8], member_name: &str) -> Result<String, AppError> {
    let reader: std::io::Cursor<&[u8]> = std::io::Cursor::new(archive);
    let mut zip: zip::ZipArchive<std::io::Cursor<&[u8]>> = zip::ZipArchive::new(reader)
        .map_err(|error| AppError::from(format!("could not open the hfd archive; [error={error}]")))?;

    let mut member: zip::read::ZipFile<'_, std::io::Cursor<&[u8]>> = zip
        .by_name(member_name)
        .map_err(|error| AppError::from(format!("the hfd archive carries no {member_name}; [error={error}]")))?;
    let mut contents: String = String::new();
    member
        .read_to_string(&mut contents)
        .map_err(|error| AppError::from(format!("{member_name} is not text; [error={error}]")))?;

    Ok(contents)
}

/// HFD's output format, as its `formats.pdf` documents it: two informational lines, a column header, then
/// space-delimited rows with an absent value written as a single `.`.
pub fn parse_fertility_file(
    contents: &str,
    columns: FertilityColumns,
) -> Result<(ParsedHfdPublication, Vec<ParsedHfdStatisticValue>), AppError> {
    let lines: Vec<&str> = contents.lines().collect();
    let header_line: &str = lines
        .get(HEADER_LINE_INDEX)
        .ok_or_else(|| AppError::from(format!(
            "the hfd cohort file has no column header; [lines={}]",
            lines.len(),
        )))?;

    let publication: ParsedHfdPublication = read_publication(&lines)?;
    let column_indices: FertilityColumnIndices = FertilityColumnIndices::from_header(header_line, columns)?;

    let mut parsed_hfd_statistic_values: Vec<ParsedHfdStatisticValue> = Vec::new();

    for line in lines.iter().skip(HEADER_LINE_INDEX + 1) {
        if line.trim().is_empty() {
            continue;
        }

        parsed_hfd_statistic_values.push(column_indices.read_row(line)?);
    }

    Ok((publication, parsed_hfd_statistic_values))
}

fn read_publication(lines: &[&str]) -> Result<ParsedHfdPublication, AppError> {
    let declared_date: &str = lines
        .iter()
        .take(HEADER_LINE_INDEX)
        .find_map(|line| line.trim().strip_prefix(LAST_MODIFIED_PREFIX))
        .ok_or_else(|| AppError::from(format!(
            "the hfd cohort file declares no {LAST_MODIFIED_PREFIX} line",
        )))?
        .trim();
    let last_modified: NaiveDate = NaiveDate::parse_from_str(declared_date, "%Y-%m-%d")
        .map_err(|error| AppError::from(format!(
            "the hfd last-modified date is unparsable; [date={declared_date} error={error}]",
        )))?;

    Ok(ParsedHfdPublication {
        revision_label: declared_date.to_string(),
        last_modified,
    })
}

fn index_of_column(names: &[&str], wanted: &str) -> Result<usize, AppError> {
    names
        .iter()
        .position(|name| *name == wanted)
        .ok_or_else(|| AppError::from(format!(
            "the hfd cohort file has no {wanted} column; [columns={}]",
            names.join(" "),
        )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_COHORT_FILE: &str = include_str!("../../samples/hfd/tfrVH.txt");
    const SAMPLE_PERIOD_FILE: &str = include_str!("../../samples/hfd/tfrRR.txt");

    fn find_value(
        parsed_hfd_statistic_values: &[ParsedHfdStatisticValue],
        hfd_code: &str,
        period_year: i32,
    ) -> ParsedHfdStatisticValue {
        parsed_hfd_statistic_values
            .iter()
            .find(|parsed| parsed.hfd_code == hfd_code && parsed.period_year == period_year)
            .cloned()
            .expect("the sample carries the requested row")
    }

    fn create_archive(members: &[(&str, &str)]) -> Vec<u8> {
        let mut writer: zip::ZipWriter<std::io::Cursor<Vec<u8>>> =
            zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();

        for (member_name, contents) in members {
            writer.start_file(*member_name, options).expect("start a member");
            std::io::Write::write_all(&mut writer, contents.as_bytes()).expect("write a member");
        }

        writer.finish().expect("finish the archive").into_inner()
    }

    #[test]
    fn parse_fertility_file_reads_the_publication_date_from_the_second_line() {
        let (publication, _): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_COHORT_FILE, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        assert_eq!(publication.revision_label, "2026-07-02");
        assert_eq!(publication.last_modified, NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
    }

    #[test]
    fn parse_fertility_file_reads_every_data_row() {
        let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_COHORT_FILE, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        assert_eq!(parsed_hfd_statistic_values.len(), 14);
    }

    #[test]
    fn parse_fertility_file_reads_the_completed_measure_not_the_age_forty_one() {
        let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_COHORT_FILE, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        let austria_1936: ParsedHfdStatisticValue = find_value(&parsed_hfd_statistic_values, "AUT", 1936);

        assert_eq!(austria_1936.value, Some(2.436));
    }

    #[test]
    fn parse_fertility_file_yields_none_for_an_absent_value() {
        let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_COHORT_FILE, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        let austria_1974: ParsedHfdStatisticValue = find_value(&parsed_hfd_statistic_values, "AUT", 1974);

        assert_eq!(austria_1974.value, None);
    }

    #[test]
    fn parse_fertility_file_keeps_codes_that_are_not_iso3() {
        let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_COHORT_FILE, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        let scotland_1930: ParsedHfdStatisticValue = find_value(&parsed_hfd_statistic_values, "GBR_SCO", 1930);

        assert_eq!(scotland_1930.value, Some(2.544));
    }

    /// The period file shares the cohort file's layout and differs only in its column names.
    #[test]
    fn parse_fertility_file_reads_the_period_file_with_its_own_columns() {
        let (publication, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(SAMPLE_PERIOD_FILE, PERIOD_FERTILITY_COLUMNS)
                .expect("parse_fertility_file succeeds");

        assert_eq!(publication.revision_label, "2026-07-02");

        let denmark_2025: ParsedHfdStatisticValue = find_value(&parsed_hfd_statistic_values, "DNK", 2025);
        assert_eq!(denmark_2025.value, Some(1.507));

        let korea_2024: ParsedHfdStatisticValue = find_value(&parsed_hfd_statistic_values, "KOR", 2024);
        assert_eq!(korea_2024.value, Some(0.749));
    }

    /// Reading the period file with the cohort columns must fail rather than silently take another column.
    #[test]
    fn parse_fertility_file_rejects_the_wrong_column_pair() {
        let error: AppError = parse_fertility_file(SAMPLE_PERIOD_FILE, COHORT_FERTILITY_COLUMNS)
            .expect_err("parse_fertility_file fails");

        assert!(error.to_string().contains("Cohort"));
    }

    #[test]
    fn parse_fertility_file_resolves_columns_by_name_not_position() {
        let reordered: &str = "Completed cohort fertility\r\n\
             Last modified: 2026-07-02\r\n\
             Code    CCF40     CCF     Cohort\r\n\
             AUT     2.402     2.436     1936\r\n";

        let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
            parse_fertility_file(reordered, COHORT_FERTILITY_COLUMNS).expect("parse_fertility_file succeeds");

        assert_eq!(parsed_hfd_statistic_values[0].period_year, 1936);
        assert_eq!(parsed_hfd_statistic_values[0].value, Some(2.436));
    }

    #[test]
    fn parse_fertility_file_rejects_a_missing_column() {
        let without_cohort: &str = "Completed cohort fertility\r\n\
             Last modified: 2026-07-02\r\n\
             Code    CCF     CCF40\r\n\
             AUT     2.436     2.402\r\n";

        let error: AppError = parse_fertility_file(without_cohort, COHORT_FERTILITY_COLUMNS).expect_err("parse_fertility_file fails");

        let message: String = error.to_string();
        assert!(message.contains("Cohort"));
        assert!(message.contains("Code CCF CCF40"));
    }

    #[test]
    fn parse_fertility_file_rejects_a_file_with_no_header() {
        let truncated: &str = "Completed cohort fertility\r\nLast modified: 2026-07-02\r\n";

        parse_fertility_file(truncated, COHORT_FERTILITY_COLUMNS).expect_err("parse_fertility_file fails");
    }

    #[test]
    fn parse_fertility_file_rejects_a_missing_last_modified_line() {
        let without_date: &str = "Completed cohort fertility\r\n\
             \r\n\
             Code    Cohort     CCF     CCF40\r\n\
             AUT     1936     2.436     2.402\r\n";

        let error: AppError = parse_fertility_file(without_date, COHORT_FERTILITY_COLUMNS).expect_err("parse_fertility_file fails");

        assert!(error.to_string().contains(LAST_MODIFIED_PREFIX));
    }

    #[test]
    fn parse_fertility_file_rejects_a_row_with_too_few_fields() {
        let short_row: &str = "Completed cohort fertility\r\n\
             Last modified: 2026-07-02\r\n\
             Code    Cohort     CCF     CCF40\r\n\
             AUT     1936\r\n";

        parse_fertility_file(short_row, COHORT_FERTILITY_COLUMNS).expect_err("parse_fertility_file fails");
    }

    #[test]
    fn parse_fertility_file_rejects_an_unparsable_value() {
        let bad_value: &str = "Completed cohort fertility\r\n\
             Last modified: 2026-07-02\r\n\
             Code    Cohort     CCF     CCF40\r\n\
             AUT     1936     n/a     2.402\r\n";

        parse_fertility_file(bad_value, COHORT_FERTILITY_COLUMNS).expect_err("parse_fertility_file fails");
    }

    #[test]
    fn read_member_reads_the_named_member_from_an_archive_of_several() {
        let archive: Vec<u8> = create_archive(&[
            (COHORT_MEMBER, SAMPLE_COHORT_FILE),
            ("tfrVHbo.txt", "by birth order"),
            (PERIOD_MEMBER, "period"),
        ]);

        assert_eq!(read_member(&archive, COHORT_MEMBER).unwrap(), SAMPLE_COHORT_FILE);
        assert_eq!(read_member(&archive, PERIOD_MEMBER).unwrap(), "period");
    }

    #[test]
    fn read_member_rejects_a_member_the_archive_does_not_carry() {
        let archive: Vec<u8> = create_archive(&[(COHORT_MEMBER, SAMPLE_COHORT_FILE)]);

        read_member(&archive, PERIOD_MEMBER).expect_err("read_member fails");
    }

    #[test]
    fn read_member_rejects_bytes_that_are_not_an_archive() {
        read_member(b"<html>not an archive</html>", COHORT_MEMBER).expect_err("read_member fails");
    }

    #[test]
    fn create_cookie_header_keeps_name_and_value_and_drops_attributes() {
        let set_cookie_values: [&str; 2] = [
            "Authorization=abc123; path=/; secure; httponly; samesite=lax",
            ".AspNetCore.Antiforgery.Xyz=tok; path=/; httponly",
        ];

        let cookie_header: String = create_cookie_header(set_cookie_values.into_iter());

        assert_eq!(cookie_header, "Authorization=abc123; .AspNetCore.Antiforgery.Xyz=tok");
    }

    #[test]
    fn create_cookie_header_is_empty_when_nothing_was_set() {
        let cookie_header: String = create_cookie_header(std::iter::empty());

        assert!(cookie_header.is_empty());
    }

    #[test]
    fn read_antiforgery_token_reads_the_value() {
        let form_html: &str = r#"<form><input name="__RequestVerificationToken" type="hidden" value="abc123" /></form>"#;

        let token: String = read_antiforgery_token(form_html).expect("read_antiforgery_token succeeds");

        assert_eq!(token, "abc123");
    }

    #[test]
    fn read_antiforgery_token_rejects_a_form_without_the_field() {
        read_antiforgery_token("<form></form>").expect_err("read_antiforgery_token fails");
    }

    #[test]
    fn read_antiforgery_token_rejects_an_unterminated_value() {
        let form_html: &str = r#"<input name="__RequestVerificationToken" value="abc123"#;

        read_antiforgery_token(form_html).expect_err("read_antiforgery_token fails");
    }
}
