//! The ISO 3166 + UN M49 country snapshot under `ingestion/db/seed-data/`, which every seed generator reads
//! to resolve a country's canonical region code from an alpha-2 or alpha-3 code.

use std::error::Error;

pub const HEADER: &str = "name,alpha-2,alpha-3,country-code,iso_3166-2,region,sub-region,intermediate-region,region-code,sub-region-code,intermediate-region-code";

#[derive(Debug)]
pub struct CountryRow {
    pub name: String,
    pub alpha_2: String,
    pub alpha_3: String,
    pub country_code: String,
    pub region_name: String,
    pub subregion_name: String,
    pub intermediate_name: String,
    pub region_code: String,
    pub subregion_code: String,
    pub intermediate_code: String,
}

pub fn parse_csv(text: &str) -> Result<Vec<CountryRow>, Box<dyn Error>> {
    let mut lines: std::str::Lines<'_> = text.lines();
    let header: &str = lines.next().ok_or("empty CSV")?;
    if header != HEADER {
        return Err(format!("unexpected CSV header: {}", header).into());
    }
    let mut country_rows: Vec<CountryRow> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<String> = parse_csv_line(line);
        if fields.len() != 11 {
            return Err(format!("expected 11 fields, got {}: {}", fields.len(), line).into());
        }
        let row: CountryRow = CountryRow {
            name:              fields[0].clone(),
            alpha_2:           fields[1].clone(),
            alpha_3:           fields[2].clone(),
            country_code:      fields[3].clone(),
            region_name:       fields[5].clone(),
            subregion_name:    fields[6].clone(),
            intermediate_name: fields[7].clone(),
            region_code:       fields[8].clone(),
            subregion_code:    fields[9].clone(),
            intermediate_code: fields[10].clone(),
        };
        if row.region_name.is_empty() {
            // UN M49 leaves the region fields blank for Antarctica and for Taiwan (which it folds into
            // China); skip both.
            continue;
        }
        country_rows.push(row);
    }
    Ok(country_rows)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    let mut current_field: String = String::new();
    let mut in_quotes: bool = false;
    for character in line.chars() {
        match (character, in_quotes) {
            ('"', _) => in_quotes = !in_quotes,
            (',', false) => {
                fields.push(std::mem::take(&mut current_field));
            }
            (other, _) => current_field.push(other),
        }
    }
    fields.push(current_field);
    fields
}
