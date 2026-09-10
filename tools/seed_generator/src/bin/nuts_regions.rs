//! Emits the dbmate seed migration for the NUTS regions Eurostat's fertility extractions name, reading one
//! JSON-stat response per level. The geo dimension of a response carries both the codes and their labels, so
//! the region set is a by-product of the same extraction the values come from. What it does not carry is
//! whether a code is still live, which the geography codelist annotates and this reads separately.
//!
//! Fetch the inputs first, `<dataset>` being `demo_r_find2` for `nuts1` and `nuts2` and `demo_r_find3` for
//! `nuts3`:
//!
//! ```sh
//! curl -sS "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data/<dataset>?format=JSON&lang=EN&geoLevel=<level>&indic_de=TOTFERRT&indic_de=AGEMOTH" -o /tmp/nuts-<level>.json
//! curl -sS "https://ec.europa.eu/eurostat/api/dissemination/sdmx/2.1/codelist/ESTAT/GEO" -o /tmp/eurostat-geo-codelist.xml
//! ```
//!
//! Then, from the repository root, `<current-nuts-revision>` being the classification in force (Eurostat marks
//! a label with a revision only once that revision has retired the code, so no input names the current one):
//!
//! ```sh
//! cargo run -p seed_generator --bin nuts_regions -- ingestion/db/seed-data/m49-iso3166-<snapshot-date>.csv /tmp/eurostat-geo-codelist.xml <current-nuts-revision> /tmp/nuts-nuts1.json /tmp/nuts-nuts2.json /tmp/nuts-nuts3.json > ingestion/db/migrations/<migration>.sql
//! ```
//!
//! No response states a region's parent. NUTS codes nest by prefix, so a region's parent is its own code
//! minus the last character, and a NUTS-1 region's is the country its two-character prefix names.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::process;

use ingestion::eurostat::eurostat_adapter;
use ingestion::eurostat::eurostat_client;
use ingestion::eurostat::eurostat_model::{EurostatDimension, EurostatResponse};

use seed_generator::country_csv::{self, CountryRow};
use seed_generator::geo_codelist::{self, CodeStanding, GeoCode};
use seed_generator::sql;

/// The tree level each input file's regions occupy, in the order the arguments give them.
const LEVEL_NAMES: [&str; 3] = ["subnational_1", "subnational_2", "subnational_3"];

const USAGE: &str = "usage: nuts_regions <m49-iso3166-csv> <geo-codelist-xml> <current-nuts-revision> \
                     <nuts1-json> <nuts2-json> <nuts3-json>";

struct SeedRegion {
    code: String,
    name_en: String,
    parent_region_code: String,
    nuts_revision: i32,
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
    let [csv_path, codelist_path, current_revision, level_paths @ ..] = arguments.as_slice()
    else {
        return Err(USAGE.into());
    };
    if level_paths.len() != LEVEL_NAMES.len() {
        return Err(USAGE.into());
    }

    let current_revision: i32 = current_revision.parse::<i32>()?;

    let csv_text: String = fs::read_to_string(csv_path)?;
    let country_rows: Vec<CountryRow> = country_csv::parse_csv(&csv_text)?;
    let region_code_by_iso2: BTreeMap<&str, String> = country_rows
        .iter()
        .map(|country_row| (country_row.alpha_2.as_str(), country_row.alpha_3.to_lowercase()))
        .collect();

    let codelist_xml: String = fs::read_to_string(codelist_path)?;
    let geo_codes: BTreeMap<String, GeoCode> = geo_codelist::parse_codelist(&codelist_xml)?;

    let responses: Vec<EurostatResponse> = level_paths
        .iter()
        .map(|path| read_response(path))
        .collect::<Result<Vec<EurostatResponse>, Box<dyn Error>>>()?;
    validate_revisions_are_retired(&responses, current_revision)?;

    let mut seeded_codes_by_level: Vec<BTreeSet<String>> = Vec::with_capacity(responses.len());
    for (level_index, response) in responses.iter().enumerate() {
        seeded_codes_by_level.push(select_codes(response, &geo_codes, level_index)?);
    }

    let mut regions_by_level: Vec<Vec<SeedRegion>> = Vec::with_capacity(responses.len());
    for (level_index, response) in responses.iter().enumerate() {
        regions_by_level.push(collect_regions(
            response,
            &seeded_codes_by_level,
            level_index,
            current_revision,
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

/// Eurostat marks a label with a revision only to tell a retired code from the live one that took its name, so
/// every marker present must be older than the revision in force. One that is not means the argument names a
/// revision Eurostat has already moved past.
fn validate_revisions_are_retired(
    responses: &[EurostatResponse],
    current_revision: i32,
) -> Result<(), Box<dyn Error>> {
    let unretired: BTreeSet<i32> = responses
        .iter()
        .flat_map(|response| eurostat_client::revision_by_geo_code(response).into_values())
        .filter(|marked| *marked >= current_revision)
        .collect();

    if !unretired.is_empty() {
        return Err(format!(
            "labels carry revisions {unretired:?}, which the given current revision {current_revision} \
             does not postdate",
        )
        .into());
    }

    Ok(())
}

/// Eurostat keeps disseminating a code after superseding it, and labels the replacement with the same revision
/// it labels the code it replaced, so only the codelist's standing separates them. An obsolete code is kept
/// where its country holds no standard code at the level: the United Kingdom left the classification, so every
/// UK code is obsolete and nothing current covers that ground.
fn select_codes(
    response: &EurostatResponse,
    geo_codes: &BTreeMap<String, GeoCode>,
    level_index: usize,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let territorial_codes: Vec<(&String, &GeoCode)> = get_territorial_codes(response, geo_codes, level_index)?;

    let countries_with_a_standard_code: BTreeSet<&str> = territorial_codes
        .iter()
        .filter(|(_, geo_code)| geo_code.standing == CodeStanding::Standard)
        .map(|(code, _)| &code[..2])
        .collect();

    let mut seeded_codes: BTreeSet<String> = BTreeSet::new();
    for (code, geo_code) in territorial_codes {
        let is_seeded: bool = match geo_code.standing {
            CodeStanding::Standard => true,
            CodeStanding::Obsolete => !countries_with_a_standard_code.contains(&code[..2]),
            CodeStanding::Unassociated => false,
        };

        if is_seeded {
            seeded_codes.insert(code.clone());
        }
    }

    Ok(seeded_codes)
}

/// The codelist states each code's level, so a response given under the wrong `geoLevel` argument is caught here.
fn get_territorial_codes<'a>(
    response: &'a EurostatResponse,
    geo_codes: &'a BTreeMap<String, GeoCode>,
    level_index: usize,
) -> Result<Vec<(&'a String, &'a GeoCode)>, Box<dyn Error>> {
    let geo: &EurostatDimension = response.dimension
        .get(eurostat_client::DIMENSION_GEO)
        .ok_or("response has no geo dimension")?;
    let expected_level: u8 = u8::try_from(level_index + 1)?;

    let mut territorial_codes: Vec<(&String, &GeoCode)> = Vec::new();
    for code in geo.category.label.keys() {
        let geo_code: &GeoCode = geo_codes
            .get(code)
            .ok_or_else(|| format!("the codelist does not carry {code}"))?;

        if geo_code.standing == CodeStanding::Unassociated {
            continue;
        }

        if geo_code.level != Some(expected_level) {
            return Err(format!(
                "{code} sits at level {:?}, not the level {expected_level} its input file holds",
                geo_code.level,
            )
            .into());
        }

        territorial_codes.push((code, geo_code));
    }

    Ok(territorial_codes)
}

fn collect_regions(
    response: &EurostatResponse,
    seeded_codes_by_level: &[BTreeSet<String>],
    level_index: usize,
    current_revision: i32,
    region_code_by_iso2: &BTreeMap<&str, String>,
) -> Result<Vec<SeedRegion>, Box<dyn Error>> {
    let geo: &EurostatDimension = response.dimension
        .get(eurostat_client::DIMENSION_GEO)
        .ok_or("response has no geo dimension")?;
    let revision_by_geo_code: BTreeMap<String, i32> = eurostat_client::revision_by_geo_code(response);

    let mut seed_regions: Vec<SeedRegion> = Vec::new();
    for (code, label) in &geo.category.label {
        if !seeded_codes_by_level[level_index].contains(code) {
            continue;
        }

        let revision: Option<&i32> = revision_by_geo_code.get(code);

        seed_regions.push(SeedRegion {
            code: code.to_lowercase(),
            name_en: name_of(label, revision.is_some()).to_string(),
            parent_region_code: get_parent_region_code(
                code,
                seeded_codes_by_level,
                level_index,
                region_code_by_iso2,
            )?,
            nuts_revision: revision.copied().unwrap_or(current_revision),
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
    seeded_codes_by_level: &[BTreeSet<String>],
    level_index: usize,
    region_code_by_iso2: &BTreeMap<&str, String>,
) -> Result<String, Box<dyn Error>> {
    if level_index == 0 {
        let iso2: &str = eurostat_adapter::get_iso2_for_geo_code(&code[..2]);

        return region_code_by_iso2
            .get(iso2)
            .cloned()
            .ok_or_else(|| format!("no seeded country for {} (from {})", iso2, code).into());
    }

    let parent_codes: &BTreeSet<String> = &seeded_codes_by_level[level_index - 1];
    let truncated: &str = &code[..code.len() - 1];
    if parent_codes.contains(truncated) {
        return Ok(truncated.to_lowercase());
    }

    get_overflowed_parent_region_code(code, parent_codes)
}

/// Northern Ireland's districts run past `UKN09` into `UKN10`, taking the character that positionally belongs
/// to the parent, so truncating that code names a region the classification never defined. Where the level
/// above holds exactly one region under the same grandparent, that region is the parent whatever the
/// characters say.
fn get_overflowed_parent_region_code(
    code: &str,
    parent_codes: &BTreeSet<String>,
) -> Result<String, Box<dyn Error>> {
    let grandparent: &str = &code[..code.len() - 2];
    let candidates: Vec<&String> = parent_codes
        .iter()
        .filter(|parent_code| parent_code.starts_with(grandparent))
        .collect();

    let [only_candidate] = candidates.as_slice()
    else {
        return Err(format!(
            "{code} names no parent by truncation, and {grandparent} holds {} regions at the level above \
             rather than one",
            candidates.len(),
        )
        .into());
    };

    Ok(only_candidate.to_lowercase())
}

fn emit_sql(regions_by_level: &[Vec<SeedRegion>], current_revision: i32) {
    println!("-- migrate:up");
    println!();
    println!("-- Eurostat demo_r_find2 (NUTS 1 and 2) and demo_r_find3 (NUTS 3), NUTS {current_revision}. A code the");
    println!("-- classification has superseded is absent, unless its country holds no current code at that level, as");
    println!("-- the United Kingdom does not. Norway sits outside the NUTS regulation and Eurostat labels its regions");
    println!("-- \"statistical region\" instead, on the same revision cycle.");
    println!("-- Names are Eurostat's own, which are endonyms for most regions even in the English extraction.");
    println!("-- Generated via tools/seed_generator: cargo run -p seed_generator --bin nuts_regions --");
    println!("--   <m49-iso3166 csv> <geo codelist xml> <current nuts revision> <nuts1 json> <nuts2 json> <nuts3 json>");
    println!("-- The binary's doc comment carries the request each input is the response to.");

    for (level_index, seed_regions) in regions_by_level.iter().enumerate() {
        emit_level(seed_regions, LEVEL_NAMES[level_index]);
    }

    emit_down();
}

fn emit_level(seed_regions: &[SeedRegion], level_name: &str) {
    println!();
    println!("with {level_name}_seed (code, name_en, parent_code, nuts_revision) as (");
    println!("    values");

    for (index, seed_region) in seed_regions.iter().enumerate() {
        let terminator: &str = if index + 1 == seed_regions.len() { "" } else { "," };
        println!(
            "        ('{}', '{}', '{}', {}){}",
            seed_region.code,
            sql::escape(&seed_region.name_en),
            seed_region.parent_region_code,
            seed_region.nuts_revision,
            terminator,
        );
    }

    println!("),");
    println!("{level_name} as (");
    println!("    insert into region (code, name_en, level, parent_region_id)");
    println!("    select {level_name}_seed.code, {level_name}_seed.name_en, '{level_name}', parent.id");
    println!("    from {level_name}_seed");
    println!("    join region as parent on parent.code = {level_name}_seed.parent_code");
    println!("    returning id, code");
    println!(")");
    println!("insert into subdivision (region_id, nuts_code, nuts_revision)");
    println!("select {level_name}.id, upper({level_name}.code), {level_name}_seed.nuts_revision");
    println!("from {level_name}");
    println!("join {level_name}_seed on {level_name}_seed.code = {level_name}.code");
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
