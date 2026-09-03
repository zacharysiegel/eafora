//! Emits the dbmate seed migration covering the UN M49 region hierarchy, the ISO 3166-1 country rows under
//! their deepest applicable parent, and the country extension table.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::process;

use seed_generator::country_csv::{self, CountryRow};
use seed_generator::sql;

fn main() {
    let result: Result<(), Box<dyn Error>> = run();
    if let Err(error) = result {
        eprintln!("seed_generator: {}", error);
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let csv_path: String = env::args().nth(1).ok_or("usage: seed_generator <csv-path>")?;
    let csv_text: String = fs::read_to_string(&csv_path)?;
    let rows: Vec<CountryRow> = country_csv::parse_csv(&csv_text)?;
    emit_sql(&rows);
    Ok(())
}

fn slugify_region(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            'a'..='z' | '0'..='9' => character,
            'A'..='Z' => character.to_ascii_lowercase(),
            ' ' | '-' => '_',
            _ => '\0',
        })
        .filter(|character| *character != '\0')
        .collect()
}

fn emit_sql(country_rows: &[CountryRow]) {
    println!("-- migrate:up");
    println!();
    println!("-- Seeds the canonical store with reference data: UN M49 hierarchy (5");
    println!("-- top-level regions, 17 subregions, 7 intermediate regions), ISO 3166-1");
    println!("-- country rows under their deepest applicable parent, the tfr statistic,");
    println!("-- and the wb_wdi data_source. Generated from");
    println!("-- ingestion/db/seed-data/m49-iso3166-2026-05-25.csv via tools/seed_generator —");
    println!("-- regenerate with `cargo run -p seed_generator -- ingestion/db/seed-data/m49-iso3166-<snapshot-date>.csv > ingestion/db/migrations/<this-file>`.");
    println!();
    emit_top_level_regions(country_rows);
    emit_subregions(country_rows);
    emit_intermediate_regions(country_rows);
    emit_countries(country_rows);
    emit_country_extensions(country_rows);
    emit_statistic();
    emit_data_source();
    emit_down();
}

fn emit_top_level_regions(country_rows: &[CountryRow]) {
    let mut top_regions: BTreeMap<String, (String, String)> = BTreeMap::new();
    for row in country_rows {
        top_regions.insert(
            row.region_name.clone(),
            (slugify_region(&row.region_name), row.region_code.clone()),
        );
    }
    println!("insert into region (code, name_en, level, m49_code) values");
    let entries: Vec<(String, String, String)> = top_regions
        .into_iter()
        .map(|(name, (code, m49))| (code, name, m49))
        .collect();
    for (index, (code, name, m49)) in entries.iter().enumerate() {
        let terminator: &str = if index + 1 == entries.len() { ";" } else { "," };
        println!(
            "    ('{}', '{}', 'region', '{}'){}",
            code,
            sql::escape(name),
            m49,
            terminator,
        );
    }
    println!();
}

fn emit_subregions(country_rows: &[CountryRow]) {
    let mut subregions: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for row in country_rows {
        if row.subregion_name.is_empty() {
            continue;
        }
        subregions.insert(
            row.subregion_name.clone(),
            (
                slugify_region(&row.subregion_name),
                row.subregion_code.clone(),
                slugify_region(&row.region_name),
            ),
        );
    }
    println!("insert into region (code, name_en, level, parent_region_id, m49_code) values");
    let entries: Vec<(String, String, String, String)> = subregions
        .into_iter()
        .map(|(name, (code, m49, parent))| (code, name, parent, m49))
        .collect();
    for (index, (code, name, parent, m49)) in entries.iter().enumerate() {
        let terminator: &str = if index + 1 == entries.len() { ";" } else { "," };
        println!(
            "    ('{}', '{}', 'subregion', (select id from region where code = '{}'), '{}'){}",
            code,
            sql::escape(name),
            parent,
            m49,
            terminator,
        );
    }
    println!();
}

fn emit_intermediate_regions(country_rows: &[CountryRow]) {
    let mut intermediates: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for row in country_rows {
        if row.intermediate_name.is_empty() {
            continue;
        }
        intermediates.insert(
            row.intermediate_name.clone(),
            (
                slugify_region(&row.intermediate_name),
                row.intermediate_code.clone(),
                slugify_region(&row.subregion_name),
            ),
        );
    }
    println!("insert into region (code, name_en, level, parent_region_id, m49_code) values");
    let entries: Vec<(String, String, String, String)> = intermediates
        .into_iter()
        .map(|(name, (code, m49, parent))| (code, name, parent, m49))
        .collect();
    for (index, (code, name, parent, m49)) in entries.iter().enumerate() {
        let terminator: &str = if index + 1 == entries.len() { ";" } else { "," };
        println!(
            "    ('{}', '{}', 'intermediate_region', (select id from region where code = '{}'), '{}'){}",
            code,
            sql::escape(name),
            parent,
            m49,
            terminator,
        );
    }
    println!();
}

fn emit_countries(country_rows: &[CountryRow]) {
    println!("insert into region (code, name_en, level, parent_region_id, m49_code) values");
    let mut sorted_rows: Vec<&CountryRow> = country_rows.iter().collect();
    sorted_rows.sort_by(|a, b| a.alpha_3.cmp(&b.alpha_3));
    for (index, row) in sorted_rows.iter().enumerate() {
        let terminator: &str = if index + 1 == sorted_rows.len() { ";" } else { "," };
        let parent_slug: String = if !row.intermediate_name.is_empty() {
            slugify_region(&row.intermediate_name)
        } else {
            slugify_region(&row.subregion_name)
        };
        let country_code: String = row.alpha_3.to_lowercase();
        println!(
            "    ('{}', '{}', 'country', (select id from region where code = '{}'), '{}'){}",
            country_code,
            sql::escape(&row.name),
            parent_slug,
            row.country_code,
            terminator,
        );
    }
    println!();
}

fn emit_country_extensions(country_rows: &[CountryRow]) {
    println!("insert into country (region_id, iso3, iso2) values");
    let mut sorted_rows: Vec<&CountryRow> = country_rows.iter().collect();
    sorted_rows.sort_by(|a, b| a.alpha_3.cmp(&b.alpha_3));
    for (index, row) in sorted_rows.iter().enumerate() {
        let terminator: &str = if index + 1 == sorted_rows.len() { ";" } else { "," };
        let country_code: String = row.alpha_3.to_lowercase();
        println!(
            "    ((select id from region where code = '{}'), '{}', '{}'){}",
            country_code, row.alpha_3, row.alpha_2, terminator,
        );
    }
    println!();
}

fn emit_statistic() {
    println!("insert into statistic (code, name_en, description, units) values");
    println!("    ('tfr', 'Total Fertility Rate', 'Average number of children that would be born to a woman over her lifetime if she experienced the current age-specific fertility rates throughout her reproductive years.', 'children per woman');");
    println!();
}

fn emit_data_source() {
    println!("insert into data_source (code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank) values");
    println!("    ('wb_wdi', 'World Bank World Development Indicators', 'https://databank.worldbank.org/source/world-development-indicators', 'attribution', 'CC BY 4.0', 'https://creativecommons.org/licenses/by/4.0/', 'World Bank, World Development Indicators (CC BY 4.0)', 100);");
    println!();
}

fn emit_down() {
    println!("-- migrate:down");
    println!();
    println!("delete from data_source where code = 'wb_wdi';");
    println!("delete from statistic   where code = 'tfr';");
    println!("delete from country;");
    println!("delete from region where level = 'country';");
    println!("delete from region where level = 'intermediate_region';");
    println!("delete from region where level = 'subregion';");
    println!("delete from region where level = 'region';");
}
