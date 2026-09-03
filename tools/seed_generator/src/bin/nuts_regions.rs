//! Emits the dbmate seed migration for the NUTS regions Eurostat's fertility extractions name, reading one
//! JSON-stat response per level. The geo dimension of a response carries both the codes and their labels, so
//! the region set is a by-product of the same extraction the values come from.
//!
//! Fetch the inputs first, `<dataset>` being `demo_r_find2` for `nuts1` and `nuts2` and `demo_r_find3` for
//! `nuts3`:
//!
//! ```sh
//! curl -sS "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data/<dataset>?format=JSON&lang=EN&geoLevel=<level>&indic_de=TOTFERRT&indic_de=AGEMOTH" -o /tmp/nuts-<level>.json
//! ```
//!
//! Then, from the repository root:
//!
//! ```sh
//! cargo run -p seed_generator --bin nuts_regions -- ingestion/db/seed-data/m49-iso3166-<snapshot-date>.csv /tmp/nuts-nuts1.json /tmp/nuts-nuts2.json /tmp/nuts-nuts3.json > ingestion/db/migrations/<migration>.sql
//! ```
//!
//! No response states a region's parent. NUTS codes nest by prefix, so a region's parent is its own code
//! minus the last character, and a NUTS-1 region's is the country its two-character prefix names.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::process;

use ingestion::eurostat::eurostat_adapter;
use ingestion::eurostat::eurostat_client;
use ingestion::eurostat::eurostat_model::{EurostatDimension, EurostatResponse};

use seed_generator::country_csv::{self, CountryRow};
use seed_generator::sql;

/// The tree level each input file's regions occupy, in the order the arguments give them.
const LEVEL_NAMES: [&str; 3] = ["subnational_1", "subnational_2", "subnational_3"];

const USAGE: &str = "usage: nuts_regions <m49-iso3166-csv> <nuts1-json> <nuts2-json> <nuts3-json>";

struct SeedRegion {
    code: String,
    name_en: String,
    parent_region_code: String,
}

fn main() {
    let result: Result<(), Box<dyn Error>> = run();
    if let Err(error) = result {
        eprintln!("nuts_regions: {}", error);
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let [csv_path, level_paths @ ..] = arguments.as_slice()
    else {
        return Err(USAGE.into());
    };
    if level_paths.len() != LEVEL_NAMES.len() {
        return Err(USAGE.into());
    }

    let csv_text: String = fs::read_to_string(csv_path)?;
    let country_rows: Vec<CountryRow> = country_csv::parse_csv(&csv_text)?;
    let region_code_by_iso2: BTreeMap<&str, String> = country_rows
        .iter()
        .map(|country_row| (country_row.alpha_2.as_str(), country_row.alpha_3.to_lowercase()))
        .collect();

    let responses: Vec<EurostatResponse> = level_paths
        .iter()
        .map(|path| read_response(path))
        .collect::<Result<Vec<EurostatResponse>, Box<dyn Error>>>()?;
    let current_revision: i32 = get_current_revision(&responses)?;

    let mut regions_by_level: Vec<Vec<SeedRegion>> = Vec::with_capacity(responses.len());
    for (level_index, response) in responses.iter().enumerate() {
        regions_by_level.push(collect_regions(
            response,
            current_revision,
            level_index,
            &region_code_by_iso2,
        )?);
    }

    emit_sql(&regions_by_level, current_revision);

    Ok(())
}

fn read_response(path: &str) -> Result<EurostatResponse, Box<dyn Error>> {
    let text: String = fs::read_to_string(path)?;
    let response: EurostatResponse = serde_json::from_str(&text)?;

    Ok(response)
}

/// A response labels each region with the revision of the classification that defines it, and carries several
/// at once, so the newest present is the one whose regions are current.
fn get_current_revision(responses: &[EurostatResponse]) -> Result<i32, Box<dyn Error>> {
    let current_revision: Option<i32> = responses
        .iter()
        .flat_map(|response| eurostat_client::revision_by_geo_code(response).into_values())
        .max();

    current_revision.ok_or_else(|| "no response labels any region with a revision".into())
}

fn collect_regions(
    response: &EurostatResponse,
    current_revision: i32,
    level_index: usize,
    region_code_by_iso2: &BTreeMap<&str, String>,
) -> Result<Vec<SeedRegion>, Box<dyn Error>> {
    let geo: &EurostatDimension = response.dimension
        .get(eurostat_client::DIMENSION_GEO)
        .ok_or("response has no geo dimension")?;
    let revision_by_geo_code: BTreeMap<String, i32> = eurostat_client::revision_by_geo_code(response);

    let mut seed_regions: Vec<SeedRegion> = Vec::new();
    for (code, label) in &geo.category.label {
        let revision: Option<&i32> = revision_by_geo_code.get(code);
        if matches!(revision, Some(&marked) if marked != current_revision) {
            continue;
        }

        seed_regions.push(SeedRegion {
            code: code.to_lowercase(),
            name_en: name_of(label, revision.is_some()).to_string(),
            parent_region_code: get_parent_region_code(code, level_index, region_code_by_iso2)?,
        });
    }

    Ok(seed_regions)
}

/// The revision marker is the label's last parenthesis, so a label carrying one names the region in what
/// precedes it.
fn name_of(label: &str, has_revision_marker: bool) -> &str {
    if !has_revision_marker {
        return label;
    }

    let opening: usize = label.rfind('(').expect("a revision marker opens a parenthesis");

    label[..opening].trim_end()
}

fn get_parent_region_code(
    code: &str,
    level_index: usize,
    region_code_by_iso2: &BTreeMap<&str, String>,
) -> Result<String, Box<dyn Error>> {
    if level_index > 0 {
        return Ok(code[..code.len() - 1].to_lowercase());
    }

    let iso2: &str = eurostat_adapter::get_iso2_for_geo_code(&code[..2]);

    region_code_by_iso2
        .get(iso2)
        .cloned()
        .ok_or_else(|| format!("no seeded country for {} (from {})", iso2, code).into())
}

fn emit_sql(regions_by_level: &[Vec<SeedRegion>], current_revision: i32) {
    println!("-- migrate:up");
    println!();
    println!("-- Eurostat demo_r_find2 (NUTS 1 and 2) and demo_r_find3 (NUTS 3), NUTS {current_revision}. Codes superseded by");
    println!("-- that revision are absent; Norway sits outside the NUTS regulation and Eurostat labels its regions");
    println!("-- \"statistical region\" instead, on the same revision cycle.");
    println!("-- Names are Eurostat's own, which are endonyms for most regions even in the English extraction.");
    println!("-- Generated via tools/seed_generator: cargo run -p seed_generator --bin nuts_regions --");
    println!("--   <m49-iso3166 csv> <nuts1 json> <nuts2 json> <nuts3 json> > <this-file>");
    println!("-- The binary's doc comment carries the request each JSON input is the response to.");

    for (level_index, seed_regions) in regions_by_level.iter().enumerate() {
        emit_level(seed_regions, LEVEL_NAMES[level_index], current_revision);
    }

    emit_down();
}

fn emit_level(seed_regions: &[SeedRegion], level_name: &str, current_revision: i32) {
    println!();
    println!("with {level_name} as (");
    println!("    insert into region (code, name_en, level, parent_region_id) values");

    for (index, seed_region) in seed_regions.iter().enumerate() {
        let terminator: &str = if index + 1 == seed_regions.len() { "" } else { "," };
        println!(
            "        ('{}', '{}', '{}', (select id from region where code = '{}')){}",
            seed_region.code,
            sql::escape(&seed_region.name_en),
            level_name,
            seed_region.parent_region_code,
            terminator,
        );
    }

    println!("    returning id, code");
    println!(")");
    println!("insert into subdivision (region_id, nuts_code, nuts_revision)");
    println!("select {level_name}.id, upper({level_name}.code), {current_revision}");
    println!("from {level_name}");
    println!(";");
}

/// Values hang off the regions this migration creates, so they go first or the delete hits a foreign key.
fn emit_down() {
    let level_list: String = LEVEL_NAMES
        .iter()
        .map(|level_name| format!("'{level_name}'"))
        .collect::<Vec<String>>()
        .join(", ");

    println!();
    println!("-- migrate:down");
    println!();
    println!("delete from statistic_value");
    println!("where region_id in (select id from region where level in ({level_list}))");
    println!(";");
    println!();
    println!("delete from subdivision");
    println!("where region_id in (select id from region where level in ({level_list}))");
    println!(";");
    println!();
    println!("delete from region");
    println!("where level in ({level_list})");
    println!(";");
}
