# Global figure in the detail panel empty state — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the World-aggregate value for the active statistic in the detail panel when no region is selected.

**Architecture:** Three stacked PRs. Phase 0 unifies the shard and geometry region key on the canonical `region.code` (dropping the `iso3`-as-key). Phase 1 seeds a supranational `world` region and keeps the World Bank `WLD` aggregate row in the WDI adapter, so the shard carries a per-year `world` value. Phase 2 has the web driver publish a default World view-model when nothing is selected, which `RegionDetailPanel` renders.

**Tech Stack:** Rust (ingestion + shared core), sqlx / Postgres (canonical store), SQLite shards, FlatGeobuf geometry, Leptos / WASM web client, dbmate migrations.

**Design:** `specs/003-web-client/global-figure-design.md`.

**Branch topology:** this plan and the design doc live on the `global-figure` planning branch. Each phase is a PR stacked linearly: Phase 0 branches off `global-figure`, Phase 1 off Phase 0, Phase 2 off Phase 1 (rebase `--onto master` as each parent squash-merges).

---

## Phase 0: unify the shard/geometry region key on `region.code`


Branch: Phase 0 branches off the `global-figure` planning branch (which carries this plan and the design doc) as `unify-region-key-on-region-code`. This is a value-behavior-preserving refactor across the `shared` and `ingestion` crates plus the `web` crate's map carriers. The map colors the same regions, hover and selection are unchanged; only the shard row key and the geometry-to-shard join key change name (and case: uppercase ISO3 to lowercase `region.code`). Because the key values change, the embedded artifact must be regenerated and re-synced (final task).

Affected repositories: this is a single monorepo (`/Users/singularity/eafora`); all changes land in one PR.

Verification commands used throughout:
- Non-render `shared`: `cargo test -p shared` (schema, geometry non-render, bundle).
- Render-gated `shared`: `cargo test -p shared --features render` (renderer, country_mesh, hit_test, shard_db).
- Ingestion: `cargo test -p ingestion` (needs `eafora_test`; run `./scripts/setup-test-db.sh` once first).
- Web wasm build: `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`.
- Web ssr build: `cargo check -p web --no-default-features --features ssr`.

### Task 1: create the Phase 0 branch

**Files:**
- Create: none (branch marker commit only)

- [ ] **Step 1: Create the branch off `global-figure` with the empty marker commit.** From the repo root, base Phase 0 on the planning branch (which holds the design doc and this plan):
  ```sh
  git -C /Users/singularity/eafora checkout global-figure && ./scripts/branch-init.sh unify-region-key-on-region-code
  ```
  `branch-init.sh` refuses if the working tree is dirty, creates `unify-region-key-on-region-code` from `global-figure`, adds the empty `>>> branch: unify-region-key-on-region-code` commit, and pushes with `-u`. Expected: the script prints the branch creation and push, and `git log --oneline -1` shows the marker subject.

### Task 2: rename the shard schema column constant to `region_code` in `shared`

**Files:**
- Modify: `shared/src/sqlite/schema.rs` (lines 1-5 module doc, line 21 constant, lines 47-57 DDL, lines 123-130 test)

- [ ] **Step 1: Run the schema test to establish the pre-rename baseline.**
  ```sh
  cargo test -p shared shard_schema_ddl_creates_expected_tables_and_index
  ```
  Expected: PASS (the test currently asserts a `region_iso3` column exists). This confirms the baseline before renaming.

- [ ] **Step 2: Rename the constant and update the DDL.** In `shared/src/sqlite/schema.rs`, replace the constant declaration (line 21):
  ```rust
  pub const COL_REGION_CODE: &str = "region_code";
  ```
  Then update the two DDL references (currently `{COL_REGION_ISO3}` on lines 48 and 56) to `{COL_REGION_CODE}`, so the column line reads `{COL_REGION_CODE} text not null,` and the primary key reads `primary key ({COL_REGION_CODE}, {COL_PERIOD_START}, {COL_PERIOD_END})`.

- [ ] **Step 3: Update the module doc comment.** In `shared/src/sqlite/schema.rs`, change the first sentence of the module doc (lines 1-5) so it no longer names `region_iso3`:
  ```rust
  //! Schema mirrors the Postgres `statistic_value` shape but is denormalized
  //! for client-side reads: `region_code` is duplicated for human-readable
  //! queries, `region_id` is kept as a BLOB for the rare cross-shard joins,
  //! periods are stored as ISO-8601 strings so client SQL doesn't need
  //! date-function support.
  ```

- [ ] **Step 4: Update the schema test.** In `shared/src/sqlite/schema.rs`, rename the local and the constant reference in `shard_schema_ddl_creates_expected_tables_and_index` (lines 123-130):
  ```rust
        let region_code_count: i64 = connection
            .query_row(
                formatcp!("select count(*) from pragma_table_info('{TABLE_STATISTIC_VALUE}') where name = '{COL_REGION_CODE}'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(region_code_count, 1);
  ```

- [ ] **Step 5: Run the schema test.**
  ```sh
  cargo test -p shared shard_schema_ddl_creates_expected_tables_and_index
  ```
  Expected: PASS. (`shard_db.rs` and the ingestion writer still reference `schema::COL_REGION_ISO3` and will fail to compile under `--features render` / in `ingestion`; those are fixed in Tasks 3 and 6. `cargo test -p shared` without `render` compiles because `shard_db.rs`'s `read_shard` is the only other consumer and its native path also references the old constant — so run only this one test by name here, which still compiles the crate. If the crate fails to compile, proceed to Step 6 and defer the run to Task 3.)

- [ ] **Step 6: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/sqlite/schema.rs && git -C /Users/singularity/eafora commit -m "shared/sqlite: rename the shard column constant COL_REGION_ISO3 to COL_REGION_CODE"
  ```

### Task 3: rekey the `ShardValues` API and `read_shard` locals on `region_code`

**Files:**
- Modify: `shared/src/sqlite/shard_db.rs` (lines 19-35 type + accessors, lines 77-119 native path, lines 201-251 wasm path, lines 265-301 test builder)

- [ ] **Step 1: Run the shard_db tests to establish the baseline.**
  ```sh
  cargo test -p shared --features render read_shard
  ```
  Expected: PASS (four `read_shard_*` tests). This is the behavior-preservation baseline; the same assertions must pass after the rename.

- [ ] **Step 2: Rename the `ShardValues` doc, accessor params, and lookup.** In `shared/src/sqlite/shard_db.rs`, update the `ShardValues` doc comment (lines 19-21) and the `value` / `cell` functions (lines 28-35). Keep the `by_region` field name:
  ```rust
  /// The values of one statistic shard, keyed by `region.code` and period start, with the min/max
  /// value range precomputed.
  #[derive(Debug, Clone)]
  pub struct ShardValues {
      by_region: HashMap<String, HashMap<NaiveDate, CellValue>>,
      min: f64,
      max: f64,
  }

  impl ShardValues {
      pub fn value(&self, region_code: &str, period_start: NaiveDate) -> Option<f64> {
          self.cell(region_code, period_start).map(|cell| cell.value)
      }

      pub fn cell(&self, region_code: &str, period_start: NaiveDate) -> Option<&CellValue> {
          self.by_region.get(region_code)?.get(&period_start)
      }
  ```

- [ ] **Step 3: Update the native `read_shard` path.** In `shared/src/sqlite/shard_db.rs`, change the doc comment on line 77 and the query column constant plus the row locals (lines 79-114). The `select` uses `schema::COL_REGION_CODE`, and the tuple binding and map insert use `region_code`:
  ```rust
      /// Read every `(region_code, period_start, value)` row of a statistic shard into a [`ShardValues`].
      /// The shard's SQLite header is validated before any query per [`crate::sqlite::schema::validate_shard_header`].
      pub fn read_shard(bytes: &[u8]) -> Result<ShardValues, AppError> {
          let connection: Connection = deserialize_read_only(bytes)?;
          schema::validate_shard_header(&connection)?;

          let query: String = format!(
              "select {}, {}, {}, {}, {} from {}",
              schema::COL_REGION_CODE,
              schema::COL_PERIOD_START,
              schema::COL_VALUE,
              schema::COL_DATA_SOURCE_CODE,
              schema::COL_DATA_SOURCE_REVISION,
              schema::TABLE_STATISTIC_VALUE,
          );

          let mut statement: rusqlite::Statement<'_> = connection.prepare(&query)?;
          let row_iter = statement.query_map([], |row| {
              let region_code: String = row.get(0)?;
              let period_start: String = row.get(1)?;
              let value: f64 = row.get(2)?;
              let source_code: String = row.get(3)?;
              let source_revision: String = row.get(4)?;

              Ok((region_code, period_start, value, source_code, source_revision))
          })?;

          let mut by_region: HashMap<String, HashMap<NaiveDate, CellValue>> = HashMap::new();
          let mut min: f64 = f64::INFINITY;
          let mut max: f64 = f64::NEG_INFINITY;

          for row in row_iter {
              let (region_code, period_start, value, source_code, source_revision): (String, String, f64, String, String) = row?;
              let period_start: NaiveDate = NaiveDate::parse_from_str(&period_start, schema::PERIOD_DATE_FORMAT)
                  .map_err(|err| AppError::from(format!("shard_db: unparseable period_start {:?}: {}", period_start, err)))?;

              let cell: CellValue = CellValue { value, source_code, source_revision };
              by_region.entry(region_code).or_default().insert(period_start, cell);
              min = min.min(value);
              max = max.max(value);
          }

          Ok(ShardValues { by_region, min, max })
      }
  ```

- [ ] **Step 4: Update the wasm `read_shard` path.** In `shared/src/sqlite/shard_db.rs`, change the query constant (line 204) and the row local (lines 230, 239) in `read_all_rows`:
  ```rust
          let query: CString = CString::new(format!(
              "select {}, {}, {}, {}, {} from {}",
              schema::COL_REGION_CODE,
              schema::COL_PERIOD_START,
              schema::COL_VALUE,
              schema::COL_DATA_SOURCE_CODE,
              schema::COL_DATA_SOURCE_REVISION,
              schema::TABLE_STATISTIC_VALUE,
          ))
          .unwrap();
  ```
  and, inside the `SQLITE_ROW` branch:
  ```rust
                  let region_code: String = ffi_conversions::column_text(statement.handle, 0)?;
  ```
  ```rust
                  by_region.entry(region_code).or_default().insert(period_start, cell);
  ```

- [ ] **Step 5: Update the native test builder.** In `shared/src/sqlite/shard_db.rs`, change the insert column constant (line 274) and the row loop binding (lines 289-295) in `sample_shard_bytes`, and lowercase the sample key values. The insert column list uses `schema::COL_REGION_CODE`, the loop binds `region_code`, and the sample rows use lowercase slugs:
  ```rust
          let insert: String = format!(
              "insert into {} ({}, {}, {}, {}, {}, {}, {}, {}) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
              schema::TABLE_STATISTIC_VALUE,
              schema::COL_REGION_CODE,
              schema::COL_REGION_ID,
              schema::COL_PERIOD_START,
              schema::COL_PERIOD_END,
              schema::COL_VALUE,
              schema::COL_DATA_STATUS,
              schema::COL_DATA_SOURCE_CODE,
              schema::COL_DATA_SOURCE_REVISION,
          );
          let region_id: Vec<u8> = vec![0u8; 16];
          let rows: [(&str, &str, &str, f64); 3] = [
              ("usa", "2020-01-01", "2020-12-31", 1.6),
              ("usa", "2021-01-01", "2021-12-31", 1.7),
              ("deu", "2020-01-01", "2020-12-31", 1.5),
          ];
          for (region_code, period_start, period_end, value) in rows {
              connection
                  .execute(
                      &insert,
                      (region_code, region_id.clone(), period_start, period_end, value, "final", "wb_wdi", "2024-12-12"),
                  )
                  .unwrap();
          }
  ```

- [ ] **Step 6: Update the native `read_shard` tests to the lowercase keys.** In `shared/src/sqlite/shard_db.rs`, change the three `#[test]` functions (lines 303-333) to query the lowercase keys (`"usa"`, `"deu"`, and the absent-region check `"xkx"`):
  ```rust
      #[test]
      fn read_shard_reads_values_and_range() {
          let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

          assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
          assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()), Some(1.7));
          assert_eq!(shard.value("deu", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.5));
          assert_eq!(shard.value_range(), Some((1.5, 1.7)));
          assert_eq!(
              shard.period_range(),
              Some((NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2021, 1, 1).unwrap())),
          );
      }

      #[test]
      fn read_shard_reads_cell_source() {
          let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

          let cell: &CellValue = shard.cell("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()).unwrap();
          assert_eq!(cell.value, 1.6);
          assert_eq!(cell.source_code, "wb_wdi");
          assert_eq!(cell.source_revision, "2024-12-12");
      }

      #[test]
      fn read_shard_returns_none_for_absent_region_and_period() {
          let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

          assert_eq!(shard.value("xkx", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), None);
          assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(1999, 1, 1).unwrap()), None);
      }
  ```

- [ ] **Step 7: Run the shard_db tests.**
  ```sh
  cargo test -p shared --features render read_shard
  ```
  Expected: PASS (same four tests as the Step 1 baseline, now against `region_code` keys). The committed wasm sample (`shared/tests/samples/tfr-sample.sqlite`) still holds uppercase keys, but the `wasm_tests` module is not exercised by this host run; it is regenerated in Task 4.

- [ ] **Step 8: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/sqlite/shard_db.rs && git -C /Users/singularity/eafora commit -m "shared/sqlite: rekey ShardValues and read_shard on region_code"
  ```

### Task 4: regenerate the committed wasm shard sample and fix its wasm test

**Files:**
- Modify: `shared/src/sqlite/shard_db.rs` (lines 356-374 wasm_tests)
- Modify (regenerated binary): `shared/tests/samples/tfr-sample.sqlite`

- [ ] **Step 1: Update the wasm sample-reading test keys.** In `shared/src/sqlite/shard_db.rs`, change `read_shard_reads_committed_sample_through_the_vfs` (lines 362-373) to the lowercase keys the regenerated sample will carry:
  ```rust
      #[wasm_bindgen_test]
      fn read_shard_reads_committed_sample_through_the_vfs() {
          let bytes: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite"));

          let shard: ShardValues = read_shard(bytes).unwrap();

          assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
          assert_eq!(shard.value("deu", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.5));
          assert_eq!(shard.value_range(), Some((1.5, 1.7)));
          assert_eq!(shard.value("xkx", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), None);
          assert_eq!(shard.cell("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()).unwrap().source_code, "wb_wdi");
      }
  ```

- [ ] **Step 2: Regenerate the committed sample from the updated builder.** The `#[ignore]`d `dump_sample_shard` test writes the sample from `sample_shard_bytes` (now lowercase-keyed after Task 3):
  ```sh
  cargo test -p shared dump_sample_shard -- --ignored --exact sqlite::shard_db::tests::dump_sample_shard
  ```
  Expected: PASS; `shared/tests/samples/tfr-sample.sqlite` is rewritten with `region_code` values `usa`/`deu`.

- [ ] **Step 3: Verify the wasm test compiles and runs against the new sample.** This is the one genuinely target-divergent surface (raw-FFI query through the read-only VFS), so it runs under the browser harness:
  ```sh
  ./scripts/test-wasm.sh
  ```
  Expected: `read_shard_reads_committed_sample_through_the_vfs` PASSES. If `test-wasm.sh` takes other arguments, inspect it first (`sed -n '1,40p' scripts/test-wasm.sh`); the required run is the `shared` package's `wasm_tests`.

- [ ] **Step 4: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/sqlite/shard_db.rs shared/tests/samples/tfr-sample.sqlite && git -C /Users/singularity/eafora commit -m "shared/sqlite: regenerate the committed wasm shard sample with region_code keys"
  ```

### Task 5: rename the render carriers `CountryMesh.iso3`, `CountrySpan.iso3`, and the fill lookup

**Files:**
- Modify: `shared/src/map/country_mesh.rs` (lines 20-40 struct + `from_feature`, lines 304-319 test)
- Modify: `shared/src/map/renderer.rs` (lines 82-89 `CountrySpan`, lines 453-454 fill lookup, lines 533-540 span construction)

- [ ] **Step 1: Run the country_mesh tests to establish the baseline.**
  ```sh
  cargo test -p shared --features render from_feature
  ```
  Expected: PASS (`from_feature_*` tests). The `from_feature_projects_the_corners_in_ring_order` test currently asserts `mesh.iso3 == "TST"`.

- [ ] **Step 2: Drop `CountryMesh.iso3`.** In `shared/src/map/country_mesh.rs`, remove the `iso3` field from the struct (line 21) and its initializer in `from_feature` (line 34). The struct becomes:
  ```rust
  #[derive(Debug, Clone)]
  pub struct CountryMesh {
      pub region_code: String,
      pub vertices: Vec<ProjectedVertexAttributes>,
      /// One per item in `vertices`: the unit direction to push that vertex to inflate the country outward
      /// (away from its interior), used to raise and outline it when hovered or selected.
      pub outward_directions: Vec<Vec2>,
      pub fill_indices: Vec<u32>,
      pub boundary_indices: Vec<u32>,
  }
  ```
  and the `from_feature` initializer:
  ```rust
          let mut mesh: CountryMesh = CountryMesh {
              region_code: feature.region_code.clone(),
              vertices: Vec::new(),
              outward_directions: Vec::new(),
              fill_indices: Vec::new(),
              boundary_indices: Vec::new(),
          };
  ```

- [ ] **Step 3: Update the country_mesh test.** In `shared/src/map/country_mesh.rs`, remove the `mesh.iso3` assertion (line 308) in `from_feature_projects_the_corners_in_ring_order`, keeping the `region_code` assertion:
  ```rust
          let mesh: CountryMesh = testland_mesh();

          assert_eq!(mesh.region_code, "testland");
          assert_eq!(mesh.vertices.len(), TESTLAND_CORNERS.len());
  ```

- [ ] **Step 4: Drop `CountrySpan.iso3` and rekey the fill lookup.** In `shared/src/map/renderer.rs`, remove the `iso3` field from `CountrySpan` (line 83), change the fill lookup (line 454) to use `span.region_code`, and drop the `iso3` initializer in `upload_country_geometry` (line 534). The struct:
  ```rust
  struct CountrySpan {
      region_code: String,
      vertex_start: u32,
      vertex_count: u32,
      fill_index_start: u32,
      fill_index_count: u32,
  }
  ```
  the lookup in `compute_fill_colors`:
  ```rust
          for span in &self.country_geometry.spans {
              let value: Option<f64> = shard_values.value(&span.region_code, frame_state.active_period_start);
  ```
  and the span construction in `upload_country_geometry`:
  ```rust
          spans.push(CountrySpan {
              region_code: country_mesh.region_code.clone(),
              vertex_start,
              vertex_count: country_mesh.vertices.len() as u32,
              fill_index_start,
              fill_index_count: country_mesh.fill_indices.len() as u32,
          });
  ```

- [ ] **Step 5: Run the render tests.**
  ```sh
  cargo test -p shared --features render
  ```
  Expected: PASS (country_mesh, renderer, hit_test, shard_db all compile and pass; the fill lookup now keys by the same `region.code` the shard is keyed on, so map coloring is unchanged).

- [ ] **Step 6: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/map/country_mesh.rs shared/src/map/renderer.rs && git -C /Users/singularity/eafora commit -m "shared/map: drop CountryMesh.iso3 and CountrySpan.iso3; key the fill lookup on region_code"
  ```

### Task 6: drop `RegionHit.iso3` in hit_test

**Files:**
- Modify: `shared/src/map/hit_test.rs` (lines 22-28 `RegionHit`, lines 57-62 construction, lines 312-317 + 477-484 tests)

- [ ] **Step 1: Run the hit_test tests to establish the baseline.**
  ```sh
  cargo test -p shared --features render region_at_point
  ```
  Expected: PASS. `assert_is_testland` currently asserts `hit.iso3 == "TST"`.

- [ ] **Step 2: Drop `RegionHit.iso3`.** In `shared/src/map/hit_test.rs`, remove the `iso3` field from `RegionHit` (line 25) and its initializer in `region_at_point` (line 59). The struct:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct RegionHit {
      pub region_code: RegionCode,
      pub name_en: String,
      pub framing: CountryFraming,
  }
  ```
  and the construction:
  ```rust
      Some(RegionHit {
          region_code: RegionCode(hit_feature.region_code.clone()),
          name_en: hit_feature.name_en.clone(),
          framing: country_framing(hit_feature),
      })
  ```

- [ ] **Step 3: Update the hit_test test helper.** In `shared/src/map/hit_test.rs`, remove the `hit.iso3` assertion from `assert_is_testland` (line 315):
  ```rust
      fn assert_is_testland(result: Option<RegionHit>) {
          let hit: RegionHit = result.expect("a region under the cursor");
          assert_eq!(hit.region_code, RegionCode("testland".to_string()));
          assert_eq!(hit.name_en, "Testland");
      }
  ```
  The `framing_feature` helper (lines 477-484) constructs a `CountryFeature`, which still carries `iso3` until Task 7; leave it unchanged here.

- [ ] **Step 4: Run the hit_test tests.**
  ```sh
  cargo test -p shared --features render region_at_point
  ```
  Expected: PASS.

- [ ] **Step 5: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/map/hit_test.rs && git -C /Users/singularity/eafora commit -m "shared/map: drop RegionHit.iso3, keying hits on region_code alone"
  ```

### Task 7: drop the geometry `iso3` feature column and `CountryFeature.iso3`

**Files:**
- Modify: `shared/src/artifact/geometry.rs` (lines 22-23 constant, lines 153-178 `CountryFeature` + `TryFrom`, lines 255-322 tests)
- Modify: `shared/src/artifact/bundle.rs` (line 188 test)
- Modify: `shared/src/map/hit_test.rs` (lines 477-485 `framing_feature` test helper)
- Modify: `ingestion/src/artifact/writer/flatgeobuf.rs` (lines 43-50 `Column` consts, lines 79-82 column declarations, lines 126-130 feature writes)

The `iso3`-overuse audit (grep confirmed) shows `CountryFeature.iso3` is consumed only by `country_mesh.rs` (now dropped, Task 5), `hit_test.rs` constructing `RegionHit.iso3` (now dropped, Task 6), and tests; it is never displayed. So the geometry `iso3` column is now vestigial and is dropped, keying geometry-to-shard on `region_code` alone.

- [ ] **Step 1: Run the geometry tests to establish the baseline.**
  ```sh
  cargo test -p shared parse_geometry_layer_parses_known_fixture features_intersecting_bbox_returns_matching_feature
  ```
  Expected: PASS (both currently assert `country_feature.iso3 == "TST"`).

- [ ] **Step 2: Drop the `iso3` column constant and `CountryFeature.iso3`.** In `shared/src/artifact/geometry.rs`, delete `FEATURE_COLUMN_ISO3` (lines 22-23), remove `iso3` from `CountryFeature` (line 155), and drop its read + initializer in the `TryFrom` (lines 166, 176). The struct:
  ```rust
  #[derive(Debug, Clone)]
  pub struct CountryFeature {
      pub name_en: String,
      pub region_code: String,
      pub polygons: Vec<Polygon>,
      pub bbox: BoundingBox,
  }
  ```
  the `TryFrom` body:
  ```rust
      fn try_from(fgb_feature: &'a FgbFeature) -> Result<Self, AppError> {
          let name_en: String = fgb_feature.property(FEATURE_COLUMN_NAME_EN)?;
          let region_code: String = fgb_feature.property(FEATURE_COLUMN_REGION_CODE)?;

          let geometry: geo_types::Geometry<f64> = fgb_feature.to_geo()?;
          let polygons: Vec<Polygon> = polygons_from_geometry(geometry)?;

          let bbox: BoundingBox = BoundingBox::from_polygons(&polygons)
              .ok_or_else(|| AppError::from("geometry feature has no coordinates".to_string()))?;

          Ok(CountryFeature { name_en, region_code, polygons, bbox })
      }
  ```

- [ ] **Step 3: Stop writing the `iso3` column in the geometry test writer.** In `shared/src/artifact/geometry.rs`, update `dump_one_feature_fgb` (lines 269-286): drop the `FEATURE_COLUMN_ISO3` column add and its property write, and re-index the remaining two columns to 0 and 1:
  ```rust
          let mut writer: FgbWriter<'_> = FgbWriter::create(GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon).unwrap();
          writer.add_column(FEATURE_COLUMN_NAME_EN, ColumnType::String, |_fbb, _col| {});
          writer.add_column(FEATURE_COLUMN_REGION_CODE, ColumnType::String, |_fbb, _col| {});
  ```
  and the feature write:
  ```rust
          writer
              .add_feature_geom(geometry, |feature| {
                  feature.property(0, FEATURE_COLUMN_NAME_EN, &ColumnValue::String("Testland")).ok();
                  feature.property(1, FEATURE_COLUMN_REGION_CODE, &ColumnValue::String("testland")).ok();
              })
              .unwrap();
  ```

- [ ] **Step 4: Update the geometry tests that assert `iso3`.** In `shared/src/artifact/geometry.rs`, update the fixture doc comment (line 255) and drop the `iso3` assertions in `parse_geometry_layer_parses_known_fixture` (line 305) and `features_intersecting_bbox_returns_matching_feature` (line 321):
  ```rust
      /// One feature: a rectangle over lon 0..2, lat 0..3, name_en "Testland" / region_code "testland".
  ```
  ```rust
          assert_eq!(country_features.len(), 1);
          let country_feature: &CountryFeature = &country_features[0];
          assert_eq!(country_feature.name_en, "Testland");
          assert_eq!(country_feature.region_code, "testland");
          assert_eq!(country_feature.polygons.len(), 1);
          assert_eq!(country_feature.bbox, BoundingBox { min_lon: 0.0, min_lat: 0.0, max_lon: 2.0, max_lat: 3.0 });
  ```
  ```rust
          assert_eq!(country_feature_hits.len(), 1);
          assert_eq!(country_feature_hits[0].region_code, "testland");
  ```

- [ ] **Step 5: Update the two dependent tests in `bundle.rs` and `hit_test.rs`.** In `shared/src/artifact/bundle.rs`, change `bundle_open_eagerly_parses_geometry` (line 188):
  ```rust
          assert_eq!(features[0].region_code, "testland");
  ```
  In `shared/src/map/hit_test.rs`, drop the `iso3` initializer from `framing_feature` (line 479):
  ```rust
      fn framing_feature(polygons: Vec<Polygon>) -> CountryFeature {
          CountryFeature {
              name_en: "Testland".to_string(),
              region_code: "testland".to_string(),
              polygons,
              bbox: BoundingBox { min_lon: 0.0, min_lat: 0.0, max_lon: 0.0, max_lat: 0.0 },
          }
      }
  ```

- [ ] **Step 6: Regenerate the committed one-feature FGB sample.** The `bundle.rs` geometry test and the `country_mesh` / `hit_test` tests read `shared/tests/samples/one-feature.fgb`, which still carries the `iso3` column. `CountryFeature::try_from` no longer reads that column, so it does not error on the stale sample, but regenerate it so the committed fixture matches the writer:
  ```sh
  cargo test -p shared dump_one_feature_fgb -- --ignored --exact artifact::geometry::tests::dump_one_feature_fgb
  ```
  Expected: PASS; `shared/tests/samples/one-feature.fgb` is rewritten with only the `name_en` and `region_code` columns.

- [ ] **Step 7: Drop the `iso3` column from the producer writer.** In `ingestion/src/artifact/writer/flatgeobuf.rs`, delete `COLUMN_ISO3` (line 43), re-index `COLUMN_NAME_EN` and `COLUMN_REGION_CODE` to 0 and 1 (lines 44-45), drop the `iso3` column add (line 80), and drop the `iso3` feature-property write (line 127):
  ```rust
  const COLUMN_NAME_EN: Column = Column { index: 0, name: geometry::FEATURE_COLUMN_NAME_EN };
  const COLUMN_REGION_CODE: Column = Column { index: 1, name: geometry::FEATURE_COLUMN_REGION_CODE };
  ```
  ```rust
      let mut writer: FgbWriter<'_> = FgbWriter::create(geometry::GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon)?;
      writer.add_column(COLUMN_NAME_EN.name, ColumnType::String, |_fbb, _col| {});
      writer.add_column(COLUMN_REGION_CODE.name, ColumnType::String, |_fbb, _col| {});
  ```
  ```rust
          writer.add_feature_geom(geo_types::Geometry::MultiPolygon(feature_polygons), |feature| {
              feature.property(COLUMN_NAME_EN.index, COLUMN_NAME_EN.name, &ColumnValue::String(&metadata.name_en)).ok();
              feature.property(COLUMN_REGION_CODE.index, COLUMN_REGION_CODE.name, &ColumnValue::String(&metadata.region_code)).ok();
          })?;
  ```
  The `read_country_iso3_to_metadata` join and its `iso3` key (used only to match Natural Earth `ADM0_A3` to a seeded country during grouping) is unchanged; that `iso3` is the genuine `Country.iso3` attribute, not the shard/geometry key.

- [ ] **Step 8: Run the shared and ingestion geometry tests.**
  ```sh
  cargo test -p shared && cargo test -p shared --features render && cargo test -p ingestion write_flatgeobuf_from_bundled_natural_earth_sample write_flatgeobuf_covers_aliased_and_merged_countries
  ```
  Expected: PASS. The two ingestion FGB tests read features back by `region_code` (they never referenced `iso3`), so they still pass with the column dropped.

- [ ] **Step 9: Commit.**
  ```sh
  git -C /Users/singularity/eafora add shared/src/artifact/geometry.rs shared/src/artifact/bundle.rs shared/src/map/hit_test.rs shared/tests/samples/one-feature.fgb ingestion/src/artifact/writer/flatgeobuf.rs && git -C /Users/singularity/eafora commit -F /tmp/phase0-task7-msg.txt
  ```
  where `/tmp/phase0-task7-msg.txt` contains (the message has backticks, so it is passed via `-F`):
  ```
  shared+ingestion: drop the vestigial `iso3` geometry feature column

  `CountryFeature.iso3` was consumed only as the shard-join key and is
  replaced by `region_code` everywhere, so stop writing and reading the
  column. The genuine `Country.iso3` attribute is untouched.
  ```

### Task 8: rename the ingestion value records to `region_code`

**Files:**
- Modify: `ingestion/src/artifact/artifact_model.rs` (lines 16-31 `CandidateValue`, lines 33-45 projection, lines 47-66 `TryFrom`, lines 68-75 `CountryMetadataProjection` doc, lines 80-106 `ResolvedValue` + `from_candidate`)
- Modify: `ingestion/src/artifact/source_choice.rs` (line 147 test)

- [ ] **Step 1: Rename `region_iso3` to `region_code` in `CandidateValue`, its projection, and the `TryFrom`.** In `ingestion/src/artifact/artifact_model.rs`, the `CandidateValue` field (line 23), the `CandidateValueProjection` field (line 36), and the `TryFrom` mapping (line 53):
  ```rust
  #[derive(Debug, Clone)]
  pub struct CandidateValue {
      pub region_id: Uuid,
      pub region_code: String,
      pub statistic_kind: StatisticKind,
      pub period: NaiveDatePeriod,
      pub value: f64,
      pub data_status: DataStatus,
      pub data_source_kind: DataSourceKind,
      pub data_source_revision: String,
      pub license_class: LicenseClass,
  }

  #[derive(Debug, Clone)]
  pub struct CandidateValueProjection {
      pub region_id: Uuid,
      pub region_code: String,
      pub statistic_code: String,
      pub period_start: NaiveDate,
      pub period_end: NaiveDate,
      pub value: f64,
      pub data_status: String,
      pub data_source_code: String,
      pub data_source_revision: String,
      pub license_class: String,
  }
  ```
  and in `TryFrom<CandidateValueProjection>`:
  ```rust
          Ok(CandidateValue {
              region_id: projection.region_id,
              region_code: projection.region_code,
              statistic_kind: StatisticKind::try_from(projection.statistic_code.as_str())?,
  ```

- [ ] **Step 2: Rename `region_iso3` in `ResolvedValue` and `from_candidate`.** In `ingestion/src/artifact/artifact_model.rs`, the `ResolvedValue` field (line 83) and the `from_candidate` mapping (line 97):
  ```rust
  #[derive(Debug, Clone)]
  pub struct ResolvedValue {
      pub region_id: Uuid,
      pub region_code: String,
      pub statistic_kind: StatisticKind,
      pub period: NaiveDatePeriod,
      pub value: f64,
      pub data_status: DataStatus,
      pub data_source_kind: DataSourceKind,
      pub data_source_revision: String,
      pub license_shard_class: LicenseShardClass,
  }
  ```
  ```rust
      pub fn from_candidate(candidate: &CandidateValue, license_shard_class: LicenseShardClass) -> Self {
          ResolvedValue {
              region_id: candidate.region_id,
              region_code: candidate.region_code.clone(),
              statistic_kind: candidate.statistic_kind,
  ```

- [ ] **Step 3: Fix the `CountryMetadataProjection` doc comment.** In `ingestion/src/artifact/artifact_model.rs` (lines 68-69), the doc still calls `iso3` "the map key" — but the map key is now `region.code`, and this projection's `iso3` is the genuine `Country.iso3` used to match Natural Earth features during grouping. Restate its purpose:
  ```rust
  /// Per-country attributes read from `country`/`region` for geometry writing: the `Country.iso3` (used
  /// to match a Natural Earth `ADM0_A3` feature to its seeded country), the English name, and the
  /// `region.code` slug written as the feature's join key.
  ```

- [ ] **Step 4: Update the `source_choice.rs` test.** In `ingestion/src/artifact/source_choice.rs`, rename the field in `make_candidate` (line 147). The value stays `"USA"` here because this test builds `CandidateValue` in-memory without touching the shard key case; but for consistency with the new key semantics use the lowercase `region.code`:
  ```rust
      fn make_candidate(data_source_kind: DataSourceKind, year: i32, value: f64) -> CandidateValue {
          CandidateValue {
              region_id: Uuid::from_u128(REGION_USA),
              region_code: "usa".to_string(),
              statistic_kind: StatisticKind::Tfr,
  ```
  (The `source_choice` resolution keys on `region_id`, not `region_code`, so the value change is inert for the test's assertions; it only aligns the field with the new naming.)

- [ ] **Step 5: Build-check ingestion (do not run yet; the db query and downsample still reference the old name).** The `artifact_db.rs` query alias and `artifact.rs` downsample comparison still use `region_iso3` and are fixed in Task 9; expect this task's changes to compile only after Task 9. Defer the compile check to Task 9, Step 5.

- [ ] **Step 6: Commit.**
  ```sh
  git -C /Users/singularity/eafora add ingestion/src/artifact/artifact_model.rs ingestion/src/artifact/source_choice.rs && git -C /Users/singularity/eafora commit -m "ingestion/artifact: rename value-record region_iso3 to region_code"
  ```

### Task 9: rekey the shard-build query on `region.code` and fix the downsample anchor

**Files:**
- Modify: `ingestion/src/artifact/artifact_db.rs` (lines 13-47 `read_candidate_values_for_statistic`)
- Modify: `ingestion/src/artifact/artifact.rs` (line 21 constant, lines 189-193 anchor, lines 252-317 tests)
- Modify (regenerated): `.sqlx/query-f4ac1ca725ba8c9929b747b073c1629c37a7610e38d505ccbdbd818569db4694.json`

- [ ] **Step 1: Rekey the candidate-value query to `region.code` via the region join.** In `ingestion/src/artifact/artifact_db.rs`, change the `country.iso3 as "region_iso3!"` select (line 22) to `region.code as "region_code!"`, and change the join from `country` to `region` (line 32). Keying by `region.code` via the region join also admits non-country regions (which have no `country` row) into the shard:
  ```rust
      let projections: Vec<CandidateValueProjection> = sqlx::query_as!(
          CandidateValueProjection,
          r#"
          select
              statistic_value.region_id              as "region_id!",
              region.code                            as "region_code!",
              statistic.code                         as "statistic_code!",
              statistic_value.period_start           as "period_start!",
              statistic_value.period_end             as "period_end!",
              statistic_value.value                  as "value!",
              statistic_value.data_status            as "data_status!",
              data_source.code                       as "data_source_code!",
              data_source_publication.revision_label as "data_source_revision!",
              data_source.license_class              as "license_class!"
          from statistic_value
          join region on region.id = statistic_value.region_id
          join statistic on statistic.id = statistic_value.statistic_id
          join data_source on data_source.id = statistic_value.data_source_id
          join data_source_publication on data_source_publication.id = statistic_value.data_source_publication_id
          where statistic_value.superseded is null
            and statistic.code = $1
          "#,
          statistic_kind.code(),
      )
      .fetch_all(executor)
      .await?;
  ```

- [ ] **Step 2: Fix the downsample reference-year anchor to the lowercase code.** In `ingestion/src/artifact/artifact.rs`, the anchor filters candidates for the United States. The constant `UNITED_STATES_ISO3 = "USA"` (line 21) is uppercase, but the key is now `region.code` (`"usa"`). Change the constant to the region code and rename it so it is not mistaken for a `Country.iso3`:
  ```rust
  const UNITED_STATES_REGION_CODE: &str = "usa";
  ```
  and the anchor filter (line 191):
  ```rust
      let reference_period_start: Option<NaiveDate> = world_bank_wdi_candidates
          .iter()
          .filter(|candidate| candidate.region_code == UNITED_STATES_REGION_CODE)
          .map(|candidate| candidate.period.start)
          .max();
  ```

- [ ] **Step 3: Update the `downsample_to_reference_year` unit tests to `region_code` and lowercase values.** In `ingestion/src/artifact/artifact.rs`, the test helper `candidate_value` (lines 252-267) and the three tests (lines 269-316) use `region_iso3` and uppercase codes. Rename the field and lowercase the codes:
  ```rust
      fn candidate_value(region_code: &str, data_source_kind: DataSourceKind, year: i32, value: f64) -> CandidateValue {
          CandidateValue {
              region_id: Uuid::now_v7(),
              region_code: region_code.to_string(),
              statistic_kind: StatisticKind::try_from("tfr").unwrap(),
              period: NaiveDatePeriod {
                  start: NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                  end: NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
              },
              value,
              data_status: DataStatus::try_from("final").unwrap(),
              data_source_kind,
              data_source_revision: "rev".to_string(),
              license_class: LicenseClass::Attribution,
          }
      }

      #[test]
      fn downsample_to_reference_year_keeps_every_region_at_the_united_states_latest_period() {
          let candidates: Vec<CandidateValue> = vec![
              candidate_value("usa", DataSourceKind::WorldBankWDI, 2021, 1.66),
              candidate_value("usa", DataSourceKind::WorldBankWDI, 2023, 1.62),
              candidate_value("deu", DataSourceKind::WorldBankWDI, 2021, 1.58),
              candidate_value("deu", DataSourceKind::WorldBankWDI, 2023, 1.46),
              candidate_value("fra", DataSourceKind::WorldBankWDI, 2023, 1.79),
              candidate_value("bra", DataSourceKind::WorldBankWDI, 2021, 1.64),
          ];

          let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

          let reference_period_start: NaiveDate = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
          assert!(kept.iter().all(|value| value.period.start == reference_period_start));
          assert!(kept.iter().any(|value| value.region_code == "usa"));
          assert!(kept.iter().any(|value| value.region_code == "deu"));
          assert!(kept.iter().any(|value| value.region_code == "fra"));
          assert!(!kept.iter().any(|value| value.region_code == "bra"));
          assert_eq!(kept.len(), 3);
      }

      #[test]
      fn downsample_to_reference_year_excludes_sources_other_than_world_bank_wdi() {
          let candidates: Vec<CandidateValue> = vec![
              candidate_value("usa", DataSourceKind::WorldBankWDI, 2023, 1.62),
              candidate_value("usa", DataSourceKind::TestAlpha, 2025, 1.50),
              candidate_value("deu", DataSourceKind::TestAlpha, 2023, 1.46),
          ];

          let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

          assert_eq!(kept.len(), 1);
          assert_eq!(kept[0].region_code, "usa");
          assert_eq!(kept[0].data_source_kind, DataSourceKind::WorldBankWDI);
          assert_eq!(kept[0].period.start, NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());
      }

      #[test]
      fn downsample_to_reference_year_yields_nothing_without_united_states_data() {
          let candidates: Vec<CandidateValue> = vec![
              candidate_value("deu", DataSourceKind::WorldBankWDI, 2023, 1.46),
          ];

          let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

          assert!(kept.is_empty());
      }
  ```

- [ ] **Step 4: Regenerate the sqlx offline cache for the changed query.** The `read_candidate_values_for_statistic` query is a `sqlx::query_as!` whose type-checked plan is cached in `.sqlx/`. After changing its columns and join, regenerate the cache (requires a reachable dev database per `.env`):
  ```sh
  ./scripts/dbmate.sh up
  ```
  `dbmate.sh` runs the migrations then `cargo sqlx prepare --workspace`, which rewrites the cached JSON (the `region_iso3`-carrying `.sqlx/query-f4ac....json` is replaced or superseded). Expected: dbmate reports "up to date" (Phase 0 adds no migration) and prints "Regenerating sqlx caches" with a successful `cargo sqlx prepare`. If `dbmate.sh` errors on the migration step but the cache is the only need, run `cargo sqlx prepare --workspace` directly against a live `DATABASE_URL`.

- [ ] **Step 5: Build and run the ingestion unit tests.**
  ```sh
  cargo test -p ingestion downsample_to_reference_year
  ```
  Expected: PASS (three `downsample_to_reference_year_*` tests). This also confirms Task 8's model rename compiles.

- [ ] **Step 6: Commit.**
  ```sh
  git -C /Users/singularity/eafora add ingestion/src/artifact/artifact_db.rs ingestion/src/artifact/artifact.rs .sqlx && git -C /Users/singularity/eafora commit -m "ingestion/artifact: key the shard-build query on region.code via the region join"
  ```

### Task 10: rekey the sqlite writer and the ingestion integration test on `region_code`

**Files:**
- Modify: `ingestion/src/artifact/writer/sqlite.rs` (lines 1-5 module doc, lines 99-131 `insert_rows`, lines 139-157 test helper, lines 159-294 tests)
- Modify: `ingestion/tests/artifact_integration.rs` (lines 116, 254, 260 shard queries)

- [ ] **Step 1: Update the sqlite writer's insert column and bind.** In `ingestion/src/artifact/writer/sqlite.rs`, change the module doc (line 2), the insert column constant in `insert_rows` (line 105), and the bind (line 117). The module doc:
  ```rust
  //! Schema mirrors the Postgres `statistic_value` shape but is denormalized
  //! for client-side reads: `region_code` is duplicated for human-readable
  //! queries, `region_id` is kept as a BLOB for the rare cross-shard joins,
  //! periods are stored as ISO-8601 strings so client SQL doesn't need
  //! date-function support.
  ```
  the insert column list:
  ```rust
      let mut statement: rusqlite::Statement = transaction.prepare(formatcp!(
          "insert into {} ({}, {}, {}, {}, {}, {}, {}, {}) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
          schema::TABLE_STATISTIC_VALUE,
          schema::COL_REGION_CODE,
          schema::COL_REGION_ID,
          schema::COL_PERIOD_START,
          schema::COL_PERIOD_END,
          schema::COL_VALUE,
          schema::COL_DATA_STATUS,
          schema::COL_DATA_SOURCE_CODE,
          schema::COL_DATA_SOURCE_REVISION,
      ))?;
  ```
  the bind:
  ```rust
      for resolved_value in values {
          statement.execute((
              &resolved_value.region_code,
              resolved_value.region_id.as_bytes().as_slice(),
              resolved_value.period.start.format(schema::PERIOD_DATE_FORMAT).to_string(),
              resolved_value.period.end.format(schema::PERIOD_DATE_FORMAT).to_string(),
              resolved_value.value,
              resolved_value.data_status.as_str(),
              resolved_value.data_source_kind.code(),
              &resolved_value.data_source_revision,
          ))?;
      }
  ```

- [ ] **Step 2: Update the sqlite writer test helper and tests.** In `ingestion/src/artifact/writer/sqlite.rs`, rename the `make_merged` parameter (lines 142, 148) and lowercase the region codes throughout, and rewrite the two `where region_iso3 = 'USA'` queries (lines 200, 209, 218) to `region_code = 'usa'`. The helper:
  ```rust
      fn make_merged(
          statistic_kind: StatisticKind,
          license_shard_class: LicenseShardClass,
          region_code: &str,
          year: i32,
          value: f64,
      ) -> ResolvedValue {
          ResolvedValue {
              region_id: Uuid::from_u128(year as u128),
              region_code: region_code.to_string(),
              statistic_kind,
              period: NaiveDatePeriod::from_year(year).unwrap(),
              value,
              data_status: DataStatus::Final,
              data_source_kind: DataSourceKind::WorldBankWDI,
              data_source_revision: "2024-Q4".to_string(),
              license_shard_class,
          }
      }
  ```
  In each test, change the `make_merged(..., "USA", ...)` / `"JPN"` calls to `"usa"` / `"jpn"`, and in `write_sqlite_shards_writes_rows_with_expected_schema` change the three lookup queries:
  ```rust
          let usa_value: f64 = connection
              .query_row(
                  "select value from statistic_value where region_code = 'usa'",
                  [],
                  |row| row.get(0),
              )
              .unwrap();
          assert!((usa_value - 1.66).abs() < f64::EPSILON);

          let usa_period_start: String = connection
              .query_row(
                  "select period_start from statistic_value where region_code = 'usa'",
                  [],
                  |row| row.get(0),
              )
              .unwrap();
          assert_eq!(usa_period_start, "2022-01-01");

          let region_id_bytes: Vec<u8> = connection
              .query_row(
                  "select region_id from statistic_value where region_code = 'usa'",
                  [],
                  |row| row.get(0),
              )
              .unwrap();
          assert_eq!(region_id_bytes.len(), 16);
  ```
  Apply the `"USA"`→`"usa"` / `"JPN"`→`"jpn"` change to the `make_merged` calls in the other four tests (`write_sqlite_shards_creates_one_file_per_statistic_per_license_class` uses `"USA"`/`"JPN"`; the three single-region tests use `"USA"`).

- [ ] **Step 3: Update the integration test's shard queries.** In `ingestion/tests/artifact_integration.rs`, the shard is opened via rusqlite and queried by the shard key column. Change the three `region_iso3` queries (lines 116, 254, 260) to `region_code` with lowercase values. The values written by the pipeline are now `region.code` slugs, so `'USA'`→`'usa'` and `'DEU'`→`'deu'`:
  ```rust
              "select value, period_start, period_end, data_status, data_source_code, data_source_revision \
               from statistic_value where region_code = 'usa'",
  ```
  ```rust
          .query_row("select count(*) from statistic_value where region_code = 'deu'", [], |row| row.get(0))
  ```
  ```rust
              "select value, period_end from statistic_value where region_code = 'usa'",
  ```
  The `get_country_region_id(&mut transaction, "USA")` helper calls (which look up the `region_id` by `Country.iso3`) stay uppercase; that is the genuine `Country.iso3` lookup, unchanged.

- [ ] **Step 4: Run the sqlite writer unit tests and the ingestion integration tests.**
  ```sh
  cargo test -p ingestion
  ```
  Expected: PASS across the crate (sqlite writer tests, `downsample_to_reference_year_*`, and the four `build_artifacts_*` / `write_flatgeobuf_*` integration tests against `eafora_test`). The value-behavior is preserved: the pipeline seeds `country` regions whose `region.code` is the lowercase alpha-3, so the shard now carries `usa`/`deu` where it carried `USA`/`DEU`.

- [ ] **Step 5: Commit.**
  ```sh
  git -C /Users/singularity/eafora add ingestion/src/artifact/writer/sqlite.rs ingestion/tests/artifact_integration.rs && git -C /Users/singularity/eafora commit -m "ingestion/artifact: bind the sqlite shard insert on region_code and lowercase the test keys"
  ```

### Task 11: rename the web `SelectionView.iso3` carrier and the driver plumbing to `region_code`

**Files:**
- Modify: `web/src/map/canvas/canvas.rs` (lines 24-32 `SelectionView`)
- Modify: `web/src/map/canvas/driver.rs` (lines 346-366 `resolve_selection_view`, lines 384-405 `select_region`, lines 591-603 `republish`)
- Modify: `web/src/map/detail_panel.rs` (line 18 destructure)

`RegionHit.iso3` was dropped in Task 6, so the driver's two `resolve_selection_view(&region_hit.iso3, ...)` call sites currently reference a field that no longer exists; those callers must pass `region_hit.region_code.0` (the `RegionCode` newtype's inner `String`). `SelectionView.iso3` is used only as the shard lookup key, in logging, and to re-resolve in `republish`; it is never displayed (the detail panel destructures it as `iso3: _`). Rename it to `region_code`.

- [ ] **Step 1: Rename `SelectionView.iso3`.** In `web/src/map/canvas/canvas.rs`, change the field (line 26):
  ```rust
  /// Published by the driver so a consumer can render the selection without bundle access.
  #[derive(Debug, Clone, PartialEq)]
  pub struct SelectionView {
      pub region_code: String,
      pub name_en: String,
      pub statistic: StatisticKind,
      pub period_start: NaiveDate,
      pub value: Option<f64>,
      pub source: Option<DataSourceKind>,
  }
  ```

- [ ] **Step 2: Rekey `resolve_selection_view` on `region_code`.** In `web/src/map/canvas/driver.rs`, rename the parameter and the shard lookup and the struct initializer (lines 346-366):
  ```rust
      fn resolve_selection_view(&self, region_code: &str, name_en: &str) -> SelectionView {
          let cell: Option<shard_db::CellValue> = self
              .read_active_shard()
              .and_then(|shard_values| shard_values.cell(region_code, self.frame_state.active_period_start).cloned());

          let value: Option<f64> = cell.as_ref().map(|cell| cell.value);
          let source: Option<DataSourceKind> = cell.as_ref().and_then(|cell| {
              DataSourceKind::try_from(cell.source_code.as_str())
                  .map_err(|error| log::warn!("shard cell has an unrecognized data source [code={} error={error}]", cell.source_code))
                  .ok()
          });

          SelectionView {
              region_code: region_code.to_string(),
              name_en: name_en.to_string(),
              statistic: self.frame_state.active_statistic,
              period_start: self.frame_state.active_period_start,
              value,
              source,
          }
      }
  ```

- [ ] **Step 3: Update `select_region`'s call site and log message.** In `web/src/map/canvas/driver.rs` (lines 395-402), pass the hit's `region_code.0` and log `region_code`:
  ```rust
          let selection_view: Option<SelectionView> =
              region_hit.map(|region_hit| self.resolve_selection_view(&region_hit.region_code.0, &region_hit.name_en));
          self.selection = selection_view.clone();

          match &selection_view {
              Some(view) => log::info!("region selected [name={} region_code={} value={:?}]", view.name_en, view.region_code, view.value),
              None => log::info!("region deselected"),
          }
  ```

- [ ] **Step 4: Update `republish`'s identity capture and re-resolve.** In `web/src/map/canvas/driver.rs` (lines 591-597), the retained-selection identity now carries `region_code`:
  ```rust
      fn republish(&mut self) -> RepublishedViews {
          let identity: Option<(String, String)> = self
              .selection
              .as_ref()
              .map(|selection| (selection.region_code.clone(), selection.name_en.clone()));
          self.selection = identity.map(|(region_code, name_en)| self.resolve_selection_view(&region_code, &name_en));
  ```

- [ ] **Step 5: Update the detail panel destructure.** In `web/src/map/detail_panel.rs` (line 18), rename the ignored field:
  ```rust
              let SelectionView { region_code: _, name_en, statistic, period_start, value, source } = selection_view;
  ```

- [ ] **Step 6: Build-check both web targets.**
  ```sh
  cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown && cargo check -p web --no-default-features --features ssr
  ```
  Expected: both PASS. The hydrate build exercises the driver (which references `RegionHit`/`SelectionView`); the ssr build never runs the renderer but still compiles the `SelectionView` type and the detail panel.

- [ ] **Step 7: Commit.**
  ```sh
  git -C /Users/singularity/eafora add web/src/map/canvas/canvas.rs web/src/map/canvas/driver.rs web/src/map/detail_panel.rs && git -C /Users/singularity/eafora commit -m "web/map: rename SelectionView.iso3 to region_code and pass the hit region_code through the driver"
  ```

### Task 12: whole-refactor verification and embedded-artifact regeneration note

**Files:**
- Modify: none (verification + a handoff note for the owner)

- [ ] **Step 1: Run the full behavior-preservation sweep.**
  ```sh
  cargo test -p shared && cargo test -p shared --features render && cargo test -p ingestion
  ```
  Expected: all PASS. Then the two web build checks:
  ```sh
  cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown && cargo check -p web --no-default-features --features ssr
  ```
  Expected: both PASS.

- [ ] **Step 2: Confirm no stray `region_iso3` / dropped-`iso3`-key references remain.**
  ```sh
  grep -rn "region_iso3\|COL_REGION_ISO3\|FEATURE_COLUMN_ISO3\|COLUMN_ISO3" /Users/singularity/eafora/ingestion/src /Users/singularity/eafora/shared/src /Users/singularity/eafora/ingestion/tests /Users/singularity/eafora/web/src
  ```
  Expected: no matches. (The genuine `Country.iso3`, `canonical_iso3`, `find_country_by_iso3`, and `read_country_iso3_to_metadata` are intentionally retained and do not match these patterns.)

- [ ] **Step 3: Regenerate and re-sync the embedded artifact.** The shard key values changed from uppercase ISO3 to lowercase `region.code`, so the committed embedded bundle under `web/static/embedded_artifacts` is now stale (its shards hold `USA`; the client reads by `region.code` `usa`). Regenerating requires the network (Natural Earth + World Bank fetch), so the owner runs it; note it in the PR. The commands are:
  ```sh
  cargo run -p ingestion -- build
  ```
  then:
  ```sh
  ./scripts/sync-embedded-bundle.sh ./web/static/embedded_artifacts
  ```
  Expected: `ingestion build` writes a new version's `downsampled/` bundle keyed on `region.code`, and `sync-embedded-bundle.sh` copies it into the web static tree. Commit the resynced bundle:
  ```sh
  git -C /Users/singularity/eafora add web/static/embedded_artifacts && git -C /Users/singularity/eafora commit -m "web: resync the embedded artifact with region.code-keyed shards"
  ```

- [ ] **Step 4: Push and open the PR.**
  ```sh
  git -C /Users/singularity/eafora push && open https://github.com/zacharysiegel/eafora/pull/new/unify-region-key-on-region-code
  ```
  PR description (write to `/tmp/phase0-pr.md`, then paste into the GitHub form):
  ```
  Unify the shard row key and the geometry-to-shard join key on the canonical `region.code` (lowercase slug), dropping the `iso3`-as-a-key misnomer that held only while every region was a country. `ShardValues`, the shard SQLite column, the FlatGeobuf feature column, and the render/hit-test/web carriers all key on `region_code`; the genuine `Country.iso3` attribute (World Bank ingest, Natural Earth matching, the `country.iso3` column) is untouched.

  Value-behavior-preserving: the map colors the same regions and selection/hover are unchanged. The shard key values change from uppercase ISO3 to lowercase `region.code`, so the embedded artifact is regenerated and resynced. Keying by the region join also lets non-country regions into the shard, which the world-region work builds on.
  ```

## Open questions / risks

- **`cargo test -p shard` does not exist.** The prompt lists "Non-render shared: `cargo test -p shard`", but the crate is named `shared` (see `shared/src/...` and `cargo test -p shared` used throughout the existing scripts/tests). I used `cargo test -p shared` (no `--features render`) for the non-render subset and `cargo test -p shared --features render` for the render-gated modules. Confirm there is no separate `shard` crate; if there is, the non-render run should target it.
- **Task ordering vs. per-task compilation.** The rename spans `schema.rs` (Task 2) before its consumers (`shard_db.rs` Task 3, `sqlite.rs` Task 10). After Task 2 the `shared` crate under `--features render` and the `ingestion` crate will not compile until Tasks 3 and 10 land, so the intermediate "run this one test by name" steps may fail to compile rather than run. I noted this inline (Task 2 Step 5, Task 8 Step 5). If strict red-green-per-step is required, an alternative is to change the constant and all its consumers within a single task; I kept them split to match the "small focused commits, one file-area per commit" convention, at the cost of a few non-compiling intermediate points. Confirm the preferred trade-off.
- **`.sqlx` cache regeneration needs a live database.** Task 9 Step 4 depends on a reachable dev DB per `.env` (`./scripts/dbmate.sh` sources `.env` and runs `cargo sqlx prepare --workspace`). If the environment has no DB, the changed `read_candidate_values_for_statistic` query will fail `cargo build`/`cargo test -p ingestion` with a stale-cache error. The owner must run this against their Mac mini Postgres.
- **`RegionCode` inner-field access `.0`.** The driver passes `region_hit.region_code.0` (Task 11). `RegionCode(pub String)` (in `frame_state.rs`) exposes the inner `String` as a public tuple field, so `.0` is valid; if a future change makes it private, an accessor would be needed. Verified `pub struct RegionCode(pub String);`.
- **Embedded-artifact regeneration is owner-run and network-dependent.** Task 12 Step 3 cannot run in this environment (fetches Natural Earth + World Bank). Until it runs, the committed embedded bundle is stale and the web client would find no values (every region reads "no data"). The PR must not merge before the resync commit lands. The exact `ingestion build` subcommand name should be verified against the ingestion CLI (`cargo run -p ingestion -- --help`); I assumed `build`.

## Phase 1: seed the world region and keep the `WLD` row in the WDI adapter


Branch: `world-region-ingest`, stacked on the Phase 0 branch `unify-region-key-on-region-code` (not master). This phase assumes Phase 0 is merged/present, so the shard-build query joins `statistic_value` to `region` and selects `region.code as "region_code!"`, the shard schema column is `COL_REGION_CODE`, and `CandidateValue`/`ResolvedValue` carry `region_code` (not `region_iso3`). All code below is written against those post-Phase-0 names.

Repositories affected: this repository only (the `eafora` monorepo). No client-repo changes; Phase 2 handles the web surface.

### Task 20: Branch setup and world-region migration

**Files:**
- Create: `ingestion/db/migrations/20260813120000_seed_world_region.sql`
- Modify: `ingestion/db/schema.sql` (regenerated by dbmate, do not hand-edit)

- [ ] **Step 1: Create the stacked branch off Phase 0 with the marker commit.** With the Phase 0 branch checked out and the working tree clean, run:

  ```sh
  ./scripts/branch-init.sh world-region-ingest
  ```

  This creates `world-region-ingest` from the current HEAD (the Phase 0 tip), makes the empty `>>> branch: world-region-ingest` commit, and pushes with upstream tracking.

- [ ] **Step 2: Write the migration seeding only the world region.** World is not a country, so seed a `region` row and no `country` row. Create `ingestion/db/migrations/20260813120000_seed_world_region.sql`:

  ```sql
  -- migrate:up

  -- The World aggregate (UN M49 code 001) is a supranational region with no country extension and no
  -- geometry. It is standalone (parent_region_id null), not wired as the parent of the five M49 top-level
  -- regions. World Bank WDI publishes its per-year figure under countryiso3code 'WLD'; the WDI adapter
  -- resolves that code to this region so the value lands as an ordinary statistic_value.
  insert into region (code, name_en, level, parent_region_id, m49_code) values
      ('world', 'World', 'world', null, '001');

  -- migrate:down

  -- statistic_value and source_choice reference region(id) without ON DELETE CASCADE, and the world
  -- region accumulates World Bank values, so clear dependents before removing the region.
  delete from statistic_value where region_id in (select id from region where code = 'world');
  delete from source_choice   where region_id in (select id from region where code = 'world');
  delete from region  where code = 'world';
  ```

- [ ] **Step 3: Apply the migration to the dev database and regenerate the sqlx cache.** No `region.level` CHECK constraint exists (the initial schema declares `level text not null` only), so `'world'` is accepted. Run:

  ```sh
  ./scripts/dbmate.sh up
  ```

  Expected: dbmate reports `Applying: 20260813120000_seed_world_region.sql`, rewrites `ingestion/db/schema.sql`, and the trailing `cargo sqlx prepare --workspace` succeeds. Confirm the row exists:

  ```sh
  psql "$DATABASE_URL" -c "select code, name_en, level, parent_region_id, m49_code from region where code = 'world';"
  ```

  Expected: one record `world | World | world | (null) | 001`.

- [ ] **Step 4: Apply the migration to the test database.** Rebuild `eafora_test` from clean so integration tests see the world region:

  ```sh
  ./scripts/setup-test-db.sh
  ```

  Expected: it drops/recreates `eafora_test` and applies all migrations through `20260813120000_seed_world_region` without error.

- [ ] **Step 5: Commit.** Backticks in the message require `-F`:

  ```sh
  git status
  git add ingestion/db/migrations/20260813120000_seed_world_region.sql ingestion/db/schema.sql
  git commit -F /tmp/eafora-commit-task20.txt
  git push
  ```

  Where `/tmp/eafora-commit-task20.txt` contains:

  ```
  ingestion: seed the `world` supranational region (M49 001)

  Standalone region row, no country extension and no geometry. The WDI
  adapter will resolve World Bank's `WLD` aggregate to this region.
  ```

### Task 21: `find_region_by_code` canonical lookup

**Files:**
- Modify: `ingestion/src/canonical/canonical_db.rs` (add helper after `find_country_by_iso3`, around line 26)
- Test: `ingestion/src/canonical/canonical_db.rs` is db-touching, so its coverage is the integration tests; the direct assertion lands in Task 22's WDI integration test. This task adds the helper and one focused integration test in the WDI test file is deferred to Task 22 — here we only compile-check.

- [ ] **Step 1: Add a failing integration test that calls the not-yet-existing helper.** Append to `ingestion/tests/world_bank_wdi_integration.rs` (after the existing `use ingestion::canonical::...`-style imports, and among the `#[tokio::test]` functions). First add the import at the top with the other `ingestion::` imports:

  ```rust
  use ingestion::canonical::canonical_db;
  ```

  Then add the test:

  ```rust
  #[tokio::test]
  async fn find_region_by_code_resolves_seeded_world_region() {
      let pool: PgPool = helpers::test_db::test_pool().await;
      let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

      let region: shared::canonical::canonical_model::Region =
          canonical_db::find_region_by_code(&mut *transaction, "world")
              .await
              .expect("find_region_by_code succeeds")
              .expect("world region is seeded");

      assert_eq!(region.code, "world");
      assert_eq!(region.level, "world");
      assert_eq!(region.m49_code.as_deref(), Some("001"));

      transaction.rollback().await.unwrap();
  }
  ```

- [ ] **Step 2: Run the test and confirm it fails to compile.**

  ```sh
  cargo test -p ingestion --test world_bank_wdi_integration find_region_by_code_resolves_seeded_world_region
  ```

  Expected: FAIL — `error[E0425]: cannot find function` / `no function or associated item named find_region_by_code` in `canonical_db`.

- [ ] **Step 3: Add the `find_region_by_code` helper.** It mirrors `find_country_by_iso3`: single query, takes `impl PgExecutor<'_>`, returns `Option<Region>`. Add the import and the function to `ingestion/src/canonical/canonical_db.rs`. First extend the model import on line 3 to include `Region`:

  ```rust
  use shared::canonical::canonical_model::{Country, DataSource, DataSourceKind, Region, Statistic};
  ```

  Extend the entity import (line 5-7) to include `RegionEntity`:

  ```rust
  use crate::canonical::canonical_entity::{
      CountryEntity, DataSourceEntity, RegionEntity, SourceChoice, SourceChoiceEntity, StatisticEntity,
  };
  ```

  Then add the function directly below `find_country_by_iso3` (after line 26):

  ```rust
  pub async fn find_region_by_code<'e>(
      executor: impl PgExecutor<'e>,
      code: &str,
  ) -> Result<Option<Region>, AppError> {
      let region_entity: Option<RegionEntity> = sqlx::query_as!(
          RegionEntity,
          r#"
          select id, code, name_en, level, parent_region_id, m49_code, created, modified
          from region
          where code = $1
          "#,
          code,
      )
      .fetch_optional(executor)
      .await?;

      Ok(region_entity.map(Region::from))
  }
  ```

- [ ] **Step 4: Regenerate the sqlx offline cache** (the new `query_as!` needs a cached entry for `cargo check` / CI offline builds):

  ```sh
  cargo sqlx prepare --workspace
  ```

  Expected: it writes/updates a `.sqlx/query-*.json` entry for the new query and reports success.

- [ ] **Step 5: Run the test and confirm it passes.**

  ```sh
  cargo test -p ingestion --test world_bank_wdi_integration find_region_by_code_resolves_seeded_world_region
  ```

  Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 6: Commit.**

  ```sh
  git status
  git add ingestion/src/canonical/canonical_db.rs ingestion/tests/world_bank_wdi_integration.rs .sqlx
  git commit -F /tmp/eafora-commit-task21.txt
  git push
  ```

  Where `/tmp/eafora-commit-task21.txt` contains:

  ```
  ingestion: add find_region_by_code canonical lookup

  Mirrors find_country_by_iso3: single query, impl PgExecutor, returns
  Option<Region>. The WDI adapter uses it to resolve the world region.
  ```

### Task 22: Special-case `WLD` in the WDI adapter's `normalize_row`

**Files:**
- Modify: `ingestion/src/world_bank_wdi/world_bank_wdi_adapter.rs` (add constants at top; branch in `normalize_row`, lines 49-76)
- Test: `ingestion/tests/world_bank_wdi_integration.rs` (add `normalize_maps_wld_to_world_region`; the existing `normalize_unknown_country_warns_and_skips` stays green)

- [ ] **Step 1: Add a failing integration test that `WLD` resolves to the world region.** Append to `ingestion/tests/world_bank_wdi_integration.rs`:

  ```rust
  #[tokio::test]
  async fn normalize_maps_wld_to_world_region() {
      let pool: PgPool = helpers::test_db::test_pool().await;
      let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

      let world_region_id: Uuid = canonical_db::find_region_by_code(&mut *transaction, "world")
          .await
          .expect("find_region_by_code succeeds")
          .expect("world region is seeded")
          .id;

      let parsed: Vec<ParsedWdiStatisticValue> = vec![ParsedWdiStatisticValue {
          iso3: "WLD".to_string(),
          year: 2024,
          value: Some(2.24),
      }];

      let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
          world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
              .await
              .expect("normalize succeeds");

      assert_eq!(normalized.len(), 1);
      assert!(warnings.is_empty());

      let normalized_statistic_value: &NormalizedStatisticValue = &normalized[0];
      assert_eq!(normalized_statistic_value.region_id, world_region_id);
      assert_eq!(normalized_statistic_value.value, 2.24);
      assert_eq!(normalized_statistic_value.period.start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());

      transaction.rollback().await.unwrap();
  }
  ```

- [ ] **Step 2: Run the new test and confirm it fails.**

  ```sh
  cargo test -p ingestion --test world_bank_wdi_integration normalize_maps_wld_to_world_region
  ```

  Expected: FAIL — the assertion `normalized.len() == 1` fails (currently `WLD` resolves through `find_country_by_iso3`, finds no country, and is dropped as an `UnknownCountry` warning, so `normalized` is empty and `warnings.len() == 1`).

- [ ] **Step 3: Add the two private constants at the top of the adapter.** Insert after the imports (after line 17) in `ingestion/src/world_bank_wdi/world_bank_wdi_adapter.rs`:

  ```rust
  /// World Bank publishes the World aggregate under this ISO-3166-shaped code. It is not a country;
  /// we map it to the canonical `world` region and drop every other WB aggregate.
  const WORLD_BANK_WORLD_CODE: &str = "WLD";

  const WORLD_REGION_CODE: &str = "world";
  ```

- [ ] **Step 4: Branch on `WLD` in `normalize_row`.** Resolve the region id up front: for `WLD`, look up the world region by code; otherwise keep the existing country resolution. Replace the country-resolution block (lines 60-68, the `let Some(country) ... else { ... UnknownCountry ... };`) with a resolved-region-id computation:

  ```rust
      let region_id: Uuid = if parsed_wdi_statistic_value.iso3 == WORLD_BANK_WORLD_CODE {
          let Some(world_region): Option<Region> =
              canonical_db::find_region_by_code(&mut *connection, WORLD_REGION_CODE).await?
          else {
              return Err(AppError::from(format!(
                  "region {:?} missing from canonical store (run dbmate up)",
                  WORLD_REGION_CODE,
              )));
          };

          world_region.id
      } else {
          let Some(country): Option<Country> =
              canonical_db::find_country_by_iso3(&mut *connection, &parsed_wdi_statistic_value.iso3).await?
          else {
              return Ok(NormalizeOutcome::Warned(IngestWarning {
                  kind: IngestWarningKind::UnknownCountry,
                  message: format!(
                      "wb_wdi: unknown countryiso3code {:?} for year {}",
                      parsed_wdi_statistic_value.iso3, parsed_wdi_statistic_value.year,
                  ),
              }));
          };

          country.region_id
      };

      Ok(NormalizeOutcome::Normalized(NormalizedStatisticValue {
          region_id,
          statistic_id,
          period: NaiveDatePeriod::from_year(parsed_wdi_statistic_value.year)?,
          value,
          data_status: DataStatus::Final,
      }))
  ```

  Add `Region` to the `shared::canonical::canonical_model` import block (lines 9-12):

  ```rust
  use shared::canonical::canonical_model::{
      Country, DataSource, DataSourceKind, DataStatus, NaiveDatePeriod, Region, SourceRevision,
      Statistic, StatisticKind,
  };
  ```

- [ ] **Step 5: Run the new test and the existing unknown-country test together; confirm both pass.**

  ```sh
  cargo test -p ingestion --test world_bank_wdi_integration normalize_maps_wld_to_world_region normalize_unknown_country_warns_and_skips
  ```

  Expected: PASS — `test result: ok. 2 passed`. `WLD` now normalizes to the world region; `ZZZ` still drops as `UnknownCountry` (it takes the `else` branch and finds no country).

- [ ] **Step 6: Commit.**

  ```sh
  git status
  git add ingestion/src/world_bank_wdi/world_bank_wdi_adapter.rs ingestion/tests/world_bank_wdi_integration.rs
  git commit -F /tmp/eafora-commit-task22.txt
  git push
  ```

  Where `/tmp/eafora-commit-task22.txt` contains:

  ```
  ingestion: resolve World Bank `WLD` to the world region in normalize

  Every other WB aggregate stays dropped as UnknownCountry; `WLD` alone
  maps to the canonical `world` region so its per-year TFR is stored.
  ```

### Task 23: Artifact shard includes the `world` entry

**Files:**
- Test: `ingestion/tests/artifact_integration.rs` (add `build_artifacts_includes_world_region_when_it_has_values`; reuses the private `insert_data_source_publication` / `insert_statistic_value` helpers already in the file)

This task adds no production code: after Phase 0, the shard-build query in `artifact_db.rs` joins `statistic_value` to `region` and keys by `region.code`, so once the world region has a value it flows into the shard automatically. The test guards that behavior end-to-end.

- [ ] **Step 1: Add a failing integration test.** Append to `ingestion/tests/artifact_integration.rs`. It inserts a World value directly against the world region id and asserts the built shard carries a `world` entry keyed by `region_code`:

  ```rust
  /// After Phase 0 keyed the shard by region.code via the region join, a supranational region with a
  /// value (the World aggregate) flows into the shard automatically, keyed 'world'. This guards that the
  /// world region is not silently dropped for lacking a country row.
  #[tokio::test]
  async fn build_artifacts_includes_world_region_when_it_has_values() {
      let pool: PgPool = test_pool().await;
      let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

      let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
      let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
      let world_region_id: Uuid = ingestion::canonical::canonical_db::find_region_by_code(&mut *transaction, "world")
          .await
          .expect("find_region_by_code succeeds")
          .expect("world region is seeded")
          .id;
      let wb_published: DateTime<Utc> = "2024-12-31T00:00:00Z".parse().unwrap();
      let publication_id: Uuid = insert_data_source_publication(&mut transaction, data_source_id, "2024-12-12", wb_published).await;
      insert_statistic_value(
          &mut transaction,
          world_region_id,
          statistic_id,
          data_source_id,
          publication_id,
          NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
          NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
          2.24,
      ).await;

      let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
      let options: BuildOptions = BuildOptions { test_offline: true };
      let build: CoupledBuildReport =
          artifact::build_artifacts(&mut *transaction, temp_dir.path(), "2026-05-26-world", options)
              .await
              .expect("build_artifacts succeeds");
      let build: BuildReport = build.complete;

      let tfr_base_shard = &build.artifacts.shards[0];
      let connection: Connection = Connection::open(&tfr_base_shard.file.path).unwrap();

      let world_value: f64 = connection
          .query_row(
              "select value from statistic_value where region_code = 'world'",
              [],
              |row| row.get(0),
          )
          .unwrap();
      assert!((world_value - 2.24).abs() < f64::EPSILON);

      transaction.rollback().await.unwrap();
  }
  ```

- [ ] **Step 2: Run the test and confirm it passes.** With Phase 0 present, the region-keyed shard build already emits the `world` row, so this test passes without production changes:

  ```sh
  cargo test -p ingestion --test artifact_integration build_artifacts_includes_world_region_when_it_has_values
  ```

  Expected: PASS (`test result: ok. 1 passed`).

  If instead it FAILS with `Query returned no rows` on the `where region_code = 'world'` lookup, Phase 0's shard-build join was not present in this branch's base; stop and reconcile with Phase 0 before proceeding (see Open questions / risks).

- [ ] **Step 3: Run the full ingestion suite to confirm nothing regressed.**

  ```sh
  cargo test -p ingestion
  ```

  Expected: PASS across `artifact_integration`, `world_bank_wdi_integration`, and `publish_integration`, including the pre-existing `normalize_unknown_country_warns_and_skips`, `build_artifacts_emits_sqlite_shard_...`, and `build_artifacts_downsampled_...` tests.

- [ ] **Step 4: Commit.**

  ```sh
  git status
  git add ingestion/tests/artifact_integration.rs
  git commit -F /tmp/eafora-commit-task23.txt
  git push
  ```

  Where `/tmp/eafora-commit-task23.txt` contains:

  ```
  ingestion: assert the shard carries the `world` region when it has values

  Guards that the region-keyed shard build (Phase 0) admits a
  supranational region with no country row.
  ```

### Task 24: Full-phase verification and PR

**Files:** none (verification + PR only).

- [ ] **Step 1: Run every gate the touched crates require.** Ingestion tests plus the workspace build (the new `query_as!` and the changed adapter compile under both web target configs since the change is in `ingestion`, but run the shared/web checks to confirm the offline sqlx cache is consistent):

  ```sh
  cargo test -p ingestion
  cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown
  cargo check -p web --no-default-features --features ssr
  ```

  Expected: all three succeed. (No `shared` render code changed this phase, so `cargo test -p shared --features render` is unaffected; run it only if the Phase 0 base is uncertain.)

- [ ] **Step 2: Self-review pass against conventions.** Confirm: explicit `let` types on every binding in the new test and adapter code; functions imported via parent module (`canonical_db::find_region_by_code`, not a bare `use`); the `Region` type imported directly; no em dashes in the migration comments or commit messages; blank lines separating the resolve-region-id phase from the `Ok(...)` return in `normalize_row`; "record" not "row" in prose; the migration's down clears `statistic_value` and `source_choice` before deleting the region.

- [ ] **Step 3: Open the PR stacked on the Phase 0 branch.**

  ```sh
  gh pr create --base unify-region-key-on-region-code --head world-region-ingest --assignee zacharysiegel --title "Seed the world region and keep WLD in the WDI adapter" --body-file /tmp/eafora-pr-body-phase1.md
  ```

  Where `/tmp/eafora-pr-body-phase1.md` contains:

  ```
  Seed a `world` supranational region (UN M49 code `001`, no country extension, no geometry) and resolve World Bank WDI's `WLD` aggregate to it in `normalize`, so the World figure is stored as an ordinary `statistic_value` and flows into the region-keyed shard as `world`. Every other WB aggregate stays dropped. Adds a `find_region_by_code` canonical lookup and integration tests covering the `WLD` mapping, the unchanged unknown-country drop, and the shard's `world` entry.

  Affects the `eafora` monorepo only. Stacked on the Phase 0 region-key refactor; Phase 2 renders the figure in the web detail panel.
  ```

## Open questions / risks

- Phase 0 is not applied in the current tree. Verified directly: `ingestion/src/artifact/artifact_db.rs:22` still selects `country.iso3 as "region_iso3!"` and joins `country`; `shared/src/sqlite/schema.rs:21` still defines `COL_REGION_ISO3 = "region_iso3"`; `CandidateValue`/`ResolvedValue` (`artifact_model.rs:23,83`) and `ShardValues` (`shared/src/sqlite/shard_db.rs`) still use `region_iso3`; and `artifact_integration.rs` still queries `where region_iso3 = 'USA'`. This entire section is written against the post-Phase-0 names (`region_code`, `COL_REGION_CODE`, region-joined shard build, `find_region_by_code`). If Phase 0's shard-build change (join `statistic_value` to `region`, select `region.code`) is not present in this branch's base, Task 23 fails at Step 2 and the world region will be absent from the shard — Phase 1 must not be branched until Phase 0's `artifact_db.rs` join lands.

- `find_region_by_code` (Task 21) may already exist if Phase 0 introduced a region lookup for the shard-build refactor. Verified it is absent today (`grep find_region_by_code` in `ingestion/src` returns nothing), but if Phase 0 adds it, skip Task 21's Step 3 and reuse the existing helper; confirm its signature matches `impl PgExecutor<'_> -> Result<Option<Region>, AppError>` before wiring Task 22 to it.

- The `ParsedWdiStatisticValue.iso3` field name is intentionally left unchanged. It is the parse-layer carrier of World Bank's raw `countryiso3code` string (a genuine `Country.iso3`-shaped value for countries, and the literal `"WLD"` for the World aggregate), so it stays `iso3` per the "iso3 survives as the genuine attribute" rule. Only the resolved canonical key (`region_code`) is renamed, and that renaming is Phase 0's scope.

- The design says World "keeps the WLD row"; there is no separate `source_choice` seeding for the world region in this phase. `source_choice.region_id` is nullable and the default (region-agnostic) choice already selects World Bank WDI for TFR, so the world region's value resolves through the existing default without a per-region `source_choice`. The migration's down still clears `source_choice` for the world region defensively (mirroring the Taiwan/Kosovo migration) in case a later phase adds one.

## Phase 2: render the World figure in the detail panel's empty state

### Task 34: Create the Phase 2 branch

**Files:**
- Create: none (branch marker commit only)

- [ ] **Step 1: Create the stacked branch off Phase 1.** With the Phase 1 branch (`world-region-ingest`) checked out and the working tree clean, run:

  ```sh
  ./scripts/branch-init.sh world-figure-web
  ```

  This creates `world-figure-web` from the current HEAD (the Phase 1 tip), makes the empty `>>> branch: world-figure-web` commit, and pushes with upstream tracking.

### Task 35: Add the "World" i18n label

**Files:**
- Modify: `web/locales/en.json` (the `detail` object, lines 9-12)

leptos_i18n generates typed keys from `en.json` at build time (`web/build.rs` runs `generate_i18n_module`), so a `t!(i18n, detail.world)` call will not compile until the key exists in the locale file. This task adds it first so later tasks can reference it.

- [ ] **Step 1: Add the `detail.world` key.** Edit `web/locales/en.json` so the `detail` object reads:

```json
    "detail": {
        "no_data": "No data for this region.",
        "source": "Source",
        "world": "World"
    },
```

- [ ] **Step 2: Confirm the key compiles into the generated i18n module.** The build script regenerates the module from the locale file; run the ssr check (fast, and it exercises `build.rs`):

```
cargo check -p web --no-default-features --features ssr
```

Expected: PASS (the new key is generated; nothing references it yet, so no unused-key error — leptos_i18n does not error on unused keys).

- [ ] **Step 3: Commit.**

```
git commit -F /tmp/eafora-phase2-task35-msg.txt web/locales/en.json
```

where `/tmp/eafora-phase2-task35-msg.txt` contains (backticks require `-F`, per zsh command-substitution):

```
web/detail-panel: add the `detail.world` i18n label for the empty-state global figure
```

---

### Task 36: Add the `GlobalView` view-model and its context

**Files:**
- Modify: `web/src/map/canvas/canvas.rs` (add `GlobalView` struct after `SelectionView`, lines 23-32; wire the new context in the `#[cfg(feature = "hydrate")]` effect, lines 55-68)
- Modify: `web/src/map/map.rs` (provide the new context, lines 10-18)

`SelectionView` carries `iso3`/`region_code` and a region name that only make sense for a selected region. Per the design, the empty state uses a dedicated view-model so the panel can distinguish "nothing selected, showing World" from "a region is selected." `GlobalView` omits any region-identity field; its label is a fixed i18n string, not `name_en`.

- [ ] **Step 1: Add the `GlobalView` struct.** In `web/src/map/canvas/canvas.rs`, immediately after the `SelectionView` definition (after line 32), add:

```rust
/// Published by the driver for the detail panel's empty state (no region selected): the world-aggregate
/// value for the active statistic and period. Distinct from `SelectionView` because it names no region;
/// the panel labels it with the fixed `detail.world` string.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalView {
    pub statistic: StatisticKind,
    pub period_start: NaiveDate,
    pub value: Option<f64>,
    pub source: Option<DataSourceKind>,
}
```

- [ ] **Step 2: Confirm the struct compiles (ssr build compiles `canvas.rs`).**

```
cargo check -p web --no-default-features --features ssr
```

Expected: PASS. (`GlobalView` is `pub` and re-exported via `pub use canvas::*` in `mod.rs`; unused-but-public items do not warn.)

- [ ] **Step 3: Provide the `GlobalView` context in `MapView`.** In `web/src/map/map.rs`, update the import on line 3 and add the context after the `legend` context (after line 18):

```rust
use crate::map::canvas::{GlobalView, LegendView, MapCanvas, SelectionView, ViewControls};
```

and, after the `legend` block:

```rust
    let global: RwSignal<Option<GlobalView>> = RwSignal::new(None);
    provide_context(global);
```

- [ ] **Step 4: Wire the `global` write-half into the driver signals.** In `web/src/map/canvas/canvas.rs`, in the `#[cfg(feature = "hydrate")]` effect (lines 55-68), read the new context and pass its write half. Replace the effect body's context reads and the `DriverSignals` literal:

```rust
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            let selection: RwSignal<Option<SelectionView>> = expect_context();
            let global: RwSignal<Option<GlobalView>> = expect_context();
            let view_controls: RwSignal<Option<ViewControls>> = expect_context();
            let legend: RwSignal<Option<LegendView>> = expect_context();
            super::driver::start(canvas, super::driver::DriverSignals {
                render_status,
                selection_view: selection.write_only(),
                global_view: global.write_only(),
                view_controls: view_controls.write_only(),
                legend: legend.write_only(),
            });
        }
    });
```

(`DriverSignals` gains `global_view` in Task 37; this step will not compile alone, so run the check after Task 37's step 1. Commit this task's files together with Task 37's first implementation step is avoided by ordering: do Step 5 below only after Task 37 exists. To keep each task independently committable, defer the `DriverSignals` field reference: instead, in this task leave `start`'s call untouched and only add the context read.)

- [ ] **Step 5 (corrected, keep this task self-contained): only add the context read, not the driver wiring.** Revert the `DriverSignals` literal in Step 4 back to its original four fields and keep just the new `global` context read as an unused binding guard. Concretely, the effect body is:

```rust
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            let selection: RwSignal<Option<SelectionView>> = expect_context();
            let _global: RwSignal<Option<GlobalView>> = expect_context();
            let view_controls: RwSignal<Option<ViewControls>> = expect_context();
            let legend: RwSignal<Option<LegendView>> = expect_context();
            super::driver::start(canvas, super::driver::DriverSignals {
                render_status,
                selection_view: selection.write_only(),
                view_controls: view_controls.write_only(),
                legend: legend.write_only(),
            });
        }
    });
```

The `_global` prefix silences the unused-variable warning; Task 37 replaces it with the real wiring.

- [ ] **Step 6: Run both checks (the effect is hydrate-only; `map.rs` compiles in both).**

```
cargo check -p web --no-default-features --features ssr
cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown
```

Expected: PASS for both. The `GlobalView` context is provided and read; the driver does not yet write it.

- [ ] **Step 7: Commit.**

```
git commit -F /tmp/eafora-phase2-task36-msg.txt web/src/map/canvas/canvas.rs web/src/map/map.rs
```

`/tmp/eafora-phase2-task36-msg.txt`:

```
web/map: add the GlobalView view-model and provide its context for the detail panel's empty state
```

---

### Task 37: Build and publish the `GlobalView` from the driver

**Files:**
- Modify: `web/src/map/canvas/canvas.rs` (final wiring of `global` into `DriverSignals`, from Task 36's deferred step)
- Modify: `web/src/map/canvas/driver.rs` (add `WORLD_REGION_CODE` const; add `global_view` to `DriverSignals` and `Driver`; add `resolve_global_view`; publish on startup, on select/deselect, and on statistic/period change)

The driver already reads the active shard and resolves a cell for a selected region in `resolve_selection_view` (lines 346-366). The world figure reuses that exact value/source lookup, keyed by the canonical `"world"` region code (the shard is keyed by `region.code` after Phase 0, and Phase 1 seeds the `world` region into the shard). The `GlobalView` is published once at startup and re-published whenever the statistic or period changes (via `RepublishedViews`), so it tracks the year scrubber like any region.

- [ ] **Step 1: Add the `WORLD_REGION_CODE` constant.** In `web/src/map/canvas/driver.rs`, in the static-constants block near the top (after `MAX_WHEEL_DELTA` / the other `const` items, before the type definitions, e.g. after line 85), add:

```rust
/// The canonical `region.code` of the World supranational region, seeded by the ingestion migration.
/// The client looks up this key in the active shard to fill the detail panel's empty state; World has
/// no geometry, so it is never a hit-test result, only this default figure.
const WORLD_REGION_CODE: &str = "world";
```

- [ ] **Step 2: Add `global_view` to `DriverSignals` and `Driver`.** In the `DriverSignals` struct (lines 617-622) add the field:

```rust
pub struct DriverSignals {
    pub render_status: RwSignal<RenderStatus>,
    pub selection_view: WriteSignal<Option<SelectionView>>,
    pub global_view: WriteSignal<Option<GlobalView>>,
    pub view_controls: WriteSignal<Option<ViewControls>>,
    pub legend: WriteSignal<Option<LegendView>>,
}
```

In the `Driver` struct (lines 179-202) add the write-signal field next to `selection_view` (line 185):

```rust
    selection_view: WriteSignal<Option<SelectionView>>,
    global_view: WriteSignal<Option<GlobalView>>,
```

Update the import on line 23 to pull in `GlobalView`:

```rust
use super::{RenderStatus, GlobalView, LegendView, SelectionView, ViewControls};
```

- [ ] **Step 3: Add `global` to `RepublishedViews` and resolve it.** Extend the `RepublishedViews` struct (lines 167-172):

```rust
struct RepublishedViews {
    view_controls: ViewControls,
    legend: LegendView,
    selection: Option<SelectionView>,
    global: GlobalView,
}
```

Add the resolver next to `resolve_selection_view` (after line 366). It reuses the same shard-cell lookup, keyed by `WORLD_REGION_CODE`:

```rust
    fn resolve_global_view(&self) -> GlobalView {
        let cell: Option<shard_db::CellValue> = self
            .read_active_shard()
            .and_then(|shard_values| shard_values.cell(WORLD_REGION_CODE, self.frame_state.active_period_start).cloned());

        let value: Option<f64> = cell.as_ref().map(|cell| cell.value);
        let source: Option<DataSourceKind> = cell.as_ref().and_then(|cell| {
            DataSourceKind::try_from(cell.source_code.as_str())
                .map_err(|error| log::warn!("shard cell has an unrecognized data source [code={} error={error}]", cell.source_code))
                .ok()
        });

        GlobalView {
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            value,
            source,
        }
    }
```

(The cell-to-value/source decode is duplicated with `resolve_selection_view`. Extract it in Step 4 rather than leaving two copies.)

- [ ] **Step 4: Extract the shared cell decode.** Both resolvers turn an `Option<CellValue>` into `(Option<f64>, Option<DataSourceKind>)`. Add one helper and call it from both. Add after `read_active_shard` (after line 344):

```rust
    /// Decodes a shard cell into a display value and its data source, warning and dropping an
    /// unrecognized source code so the panel shows the value without attribution rather than nothing.
    fn decode_cell(cell: Option<shard_db::CellValue>) -> (Option<f64>, Option<DataSourceKind>) {
        let value: Option<f64> = cell.as_ref().map(|cell| cell.value);
        let source: Option<DataSourceKind> = cell.as_ref().and_then(|cell| {
            DataSourceKind::try_from(cell.source_code.as_str())
                .map_err(|error| log::warn!("shard cell has an unrecognized data source [code={} error={error}]", cell.source_code))
                .ok()
        });

        (value, source)
    }
```

Rewrite `resolve_selection_view` (lines 346-366) to use it. Note this signature already takes `region_code` per Phase 0:

```rust
    fn resolve_selection_view(&self, region_code: &str, name_en: &str) -> SelectionView {
        let cell: Option<shard_db::CellValue> = self
            .read_active_shard()
            .and_then(|shard_values| shard_values.cell(region_code, self.frame_state.active_period_start).cloned());

        let (value, source): (Option<f64>, Option<DataSourceKind>) = Self::decode_cell(cell);

        SelectionView {
            name_en: name_en.to_string(),
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            value,
            source,
        }
    }
```

And rewrite `resolve_global_view`'s decode to call the helper:

```rust
    fn resolve_global_view(&self) -> GlobalView {
        let cell: Option<shard_db::CellValue> = self
            .read_active_shard()
            .and_then(|shard_values| shard_values.cell(WORLD_REGION_CODE, self.frame_state.active_period_start).cloned());

        let (value, source): (Option<f64>, Option<DataSourceKind>) = Self::decode_cell(cell);

        GlobalView {
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            value,
            source,
        }
    }
```

(Per Phase 0, `SelectionView` no longer has an `iso3` field — the destructure in `select_region` at line 396 becomes `self.resolve_selection_view(&region_hit.region_code.0, &region_hit.name_en)` and `republish` at lines 592-596 tracks `region_code` instead of `iso3`. Those edits belong to Phase 0; this task assumes them done. If they are not, see Open questions.)

- [ ] **Step 5: Populate `global` in `republish`.** Update `republish` (lines 591-603) so it resolves the world view alongside the selection:

```rust
    fn republish(&mut self) -> RepublishedViews {
        let identity: Option<(String, String)> = self
            .selection
            .as_ref()
            .map(|selection| (selection.name_en.clone(), selection.name_en.clone()));
        self.selection = self.selection.as_ref().map(|selection| {
            self.resolve_selection_view(&selection.name_en, &selection.name_en)
        });

        RepublishedViews {
            view_controls: self.view_controls(),
            legend: self.legend_view(),
            selection: self.selection.clone(),
            global: self.resolve_global_view(),
        }
    }
```

Note: `republish`'s selection re-resolution needs the selected region's `region_code`, which `SelectionView` no longer stores after Phase 0. The retained key must come from `self.frame_state.selected_region` (a `RegionCode`), paired with the retained `name_en`. Rewrite it precisely:

```rust
    fn republish(&mut self) -> RepublishedViews {
        self.selection = match (self.frame_state.selected_region.as_ref(), self.selection.as_ref()) {
            (Some(region_code), Some(selection)) => {
                Some(self.resolve_selection_view(&region_code.0, &selection.name_en))
            },
            _ => None,
        };

        RepublishedViews {
            view_controls: self.view_controls(),
            legend: self.legend_view(),
            selection: self.selection.clone(),
            global: self.resolve_global_view(),
        }
    }
```

- [ ] **Step 6: Publish `global` in `publish_mutation`.** In `publish_mutation` (lines 989-1014), extend `PendingPublish` and the set calls to carry the global signal:

```rust
    struct PendingPublish {
        controls_signal: WriteSignal<Option<ViewControls>>,
        selection_signal: WriteSignal<Option<SelectionView>>,
        global_signal: WriteSignal<Option<GlobalView>>,
        legend_signal: WriteSignal<Option<LegendView>>,
        views: RepublishedViews,
    }

    let pending: Option<PendingPublish> = with_driver(|driver| {
        let views: RepublishedViews = mutate(driver)?;

        Some(PendingPublish {
            controls_signal: driver.view_controls,
            selection_signal: driver.selection_view,
            global_signal: driver.global_view,
            legend_signal: driver.legend,
            views,
        })
    })
    .flatten();

    if let Some(pending) = pending {
        pending.controls_signal.set(Some(pending.views.view_controls));
        pending.selection_signal.set(pending.views.selection);
        pending.global_signal.set(Some(pending.views.global));
        pending.legend_signal.set(Some(pending.views.legend));
    }
```

- [ ] **Step 7: Wire and publish the initial `global` in `start`/`set_up_driver`.** In `start` (lines 624-642) destructure the new field and thread it into `set_up_driver`:

```rust
pub fn start(canvas: HtmlCanvasElement, signals: DriverSignals) {
    let DriverSignals { render_status, selection_view, global_view, view_controls, legend } = signals;

    leptos::task::spawn_local(async move {
        let status: RenderStatus = match set_up_driver(canvas, selection_view, global_view, view_controls, legend).await {
            Ok(()) => RenderStatus::Ready,
            Err(StartupError::DataUnavailable(error)) => {
                log::error!("map data could not be loaded [error={error}]");
                RenderStatus::DataUnavailable
            }
            Err(StartupError::BrowserUnsupported(error)) => {
                log::error!("browser is missing a required capability, showing the unsupported panel [error={error}]");
                RenderStatus::Unsupported
            }
        };

        render_status.set(status);
    });
}
```

Update `set_up_driver`'s signature and body (lines 644-707) to accept `global_view`, store it on the `Driver`, and publish the initial world figure alongside the initial controls and legend:

```rust
async fn set_up_driver(canvas: HtmlCanvasElement, selection_view: WriteSignal<Option<SelectionView>>, global_view: WriteSignal<Option<GlobalView>>, view_controls: WriteSignal<Option<ViewControls>>, legend: WriteSignal<Option<LegendView>>) -> Result<(), StartupError> {
```

In the `Driver { ... }` literal (lines 671-694) add the field next to `selection_view`:

```rust
        selection_view,
        global_view,
```

After computing `initial_controls` and `initial_legend` (lines 696-697), add:

```rust
    let initial_controls: ViewControls = driver.view_controls();
    let initial_legend: LegendView = driver.legend_view();
    let initial_global: GlobalView = driver.resolve_global_view();
```

and after the existing `view_controls.set` / `legend.set` (lines 703-704) add:

```rust
    view_controls.set(Some(initial_controls));
    legend.set(Some(initial_legend));
    global_view.set(Some(initial_global));
```

- [ ] **Step 8: Complete the deferred `canvas.rs` wiring from Task 36.** In `web/src/map/canvas/canvas.rs`, replace the `_global` guard binding and the four-field `DriverSignals` literal with the real wiring:

```rust
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            let selection: RwSignal<Option<SelectionView>> = expect_context();
            let global: RwSignal<Option<GlobalView>> = expect_context();
            let view_controls: RwSignal<Option<ViewControls>> = expect_context();
            let legend: RwSignal<Option<LegendView>> = expect_context();
            super::driver::start(canvas, super::driver::DriverSignals {
                render_status,
                selection_view: selection.write_only(),
                global_view: global.write_only(),
                view_controls: view_controls.write_only(),
                legend: legend.write_only(),
            });
        }
    });
```

- [ ] **Step 9: Run both checks.** `driver.rs` compiles only under `hydrate`; `canvas.rs` compiles in both.

```
cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown
cargo check -p web --no-default-features --features ssr
```

Expected: PASS for both. (If ssr fails on the `GlobalView` import in `canvas.rs`, confirm Task 36 re-exported it via `pub use canvas::*`; it does.)

- [ ] **Step 10: Commit.**

```
git commit -F /tmp/eafora-phase2-task37-msg.txt web/src/map/canvas/driver.rs web/src/map/canvas/canvas.rs
```

`/tmp/eafora-phase2-task37-msg.txt`:

```
web/map: publish the World figure from the driver on startup and on statistic/period change

The driver reads the `world` region entry from the active shard, reusing the
same cell decode as a selected region, and publishes it as a GlobalView.
```

---

### Task 38: Render the World figure in the detail panel's empty state

**Files:**
- Modify: `web/src/map/detail_panel.rs` (render `GlobalView` when `selection` is `None`, whole file)

Today `RegionDetailPanel` renders `SelectionView` when `Some` and is blank when `None` (lines 16-45). The empty state now reads the `GlobalView` context and renders the same markup and classes, labeled "World" via `detail.world`. The value/unit/source sub-tree is identical to the region case, so it is extracted into one helper shared by both.

- [ ] **Step 1: Rewrite `RegionDetailPanel` to render the World default when nothing is selected.** Replace the whole file with:

```rust
use chrono::Datelike;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::{GlobalView, SelectionView};
use crate::map::labels;

#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let global: RwSignal<Option<GlobalView>> = expect_context();
    let i18n = use_i18n();

    move || match selection.get() {
        Some(selection_view) => {
            let SelectionView { name_en, statistic, period_start, value, source } = selection_view;

            Some(detail_panel(i18n, name_en, statistic, period_start.year(), value, source))
        },
        None => global.get().map(|global_view| {
            let GlobalView { statistic, period_start, value, source } = global_view;
            let world_label: String = t_string!(i18n, detail.world).to_string();

            detail_panel(i18n, world_label, statistic, period_start.year(), value, source)
        }),
    }
}

fn detail_panel(
    i18n: I18nContext<Locale>,
    region_label: String,
    statistic: StatisticKind,
    year: i32,
    value: Option<f64>,
    source: Option<DataSourceKind>,
) -> impl IntoView {
    view! {
        <aside class="panel detail-panel">
            <p class="detail-panel-region">{region_label}</p>
            <p class="detail-panel-statistic">
                {labels::statistic_label(i18n, statistic)}
                " · "
                {year.to_string()}
            </p>
            {match value {
                Some(value) => view! {
                    <p class="detail-panel-value numeric">{format!("{value:.2}")}</p>
                    <p class="detail-panel-unit">{labels::statistic_unit(i18n, statistic)}</p>
                    {source.map(|source| view! {
                        <p class="detail-panel-source">{t!(i18n, detail.source)} ": " {source_label(i18n, source)}</p>
                    })}
                }
                .into_any(),
                None => view! {
                    <p class="detail-panel-no-data">{t!(i18n, detail.no_data)}</p>
                }
                .into_any(),
            }}
        </aside>
    }
}

fn source_label(i18n: I18nContext<Locale>, source: DataSourceKind) -> AnyView {
    match source {
        DataSourceKind::WorldBankWDI => t!(i18n, source.wb_wdi).into_any(),
        // test-only variants; never present in production shards, so these arms only satisfy match exhaustiveness
        DataSourceKind::TestAlpha => source.code().into_any(),
        DataSourceKind::TestBeta => source.code().into_any(),
    }
}
```

Notes on the mechanical points:
- The outer closure returns `Option<_>` in both arms (the `Some` arm wraps `detail_panel(...)` in `Some`, the `None` arm's `.map` already yields `Option<_>`), preserving the original "returns an `Option` view" shape the reactive closure expects.
- `t_string!` is leptos_i18n's macro for getting the string value of a key (needed because `region_label` is a `String` passed by value, whereas the region case passes `name_en: String`). If `t_string!` is not in scope via `use crate::i18n::*;`, use the equivalent already-imported form; verify against a sibling call site.

- [ ] **Step 2: Confirm the `t_string!` form.** Before running the check, grep for an existing string-valued i18n use to confirm the macro name in this leptos_i18n version:

```
grep -rn "t_string!\|td_string!\|t!(" web/src/ | grep -iv target | head
```

If `t_string!` does not appear anywhere and the check in Step 3 errors on it, the portable fallback is to render the label through the same `t!` path as other strings by inlining it in the view for the World arm instead of passing a `String`. In that case, split `detail_panel` so the region-label node is a parameter of type `AnyView`: the region arm passes `name_en.into_any()`, the World arm passes `t!(i18n, detail.world).into_any()`. This avoids needing the string value at all. Prefer this `AnyView` form if `t_string!` is uncertain:

```rust
fn detail_panel(
    i18n: I18nContext<Locale>,
    region_label: AnyView,
    statistic: StatisticKind,
    year: i32,
    value: Option<f64>,
    source: Option<DataSourceKind>,
) -> impl IntoView {
    view! {
        <aside class="panel detail-panel">
            <p class="detail-panel-region">{region_label}</p>
            // ... unchanged ...
        </aside>
    }
}
```

with call sites `detail_panel(i18n, name_en.into_any(), ...)` and `detail_panel(i18n, t!(i18n, detail.world).into_any(), ...)`. Choose one form (the `AnyView` form is the safer default) and use it consistently.

- [ ] **Step 3: Run both checks (`detail_panel.rs` compiles in both builds).**

```
cargo check -p web --no-default-features --features ssr
cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown
```

Expected: PASS for both.

- [ ] **Step 4: Commit.**

```
git commit -F /tmp/eafora-phase2-task38-msg.txt web/src/map/detail_panel.rs
```

`/tmp/eafora-phase2-task38-msg.txt`:

```
web/detail-panel: render the World figure in the empty state, extracting the shared value/source markup
```

---

### Task 39: Manual verification in Chrome

**Files:**
- No source changes. Manual browser verification, per the wasm-test convention (target-agnostic reactive glue is covered by `cargo check`; a browser harness would add cost, not coverage).

The feature has no wasm-vs-host divergence in its logic, so there is no `#[wasm_bindgen_test]` to add. The behavior that must be observed live is the reactive plumbing (context propagation, signal republish on scrub/statistic change) plus the presence of a `world` shard entry, which only the running app exercises end to end.

- [ ] **Step 1: Rebuild the embedded artifact so the running app has a `world` shard entry.** Phases 0-1 changed the shard key and seeded the world region; the web app embeds a prebuilt bundle. Regenerate it per the project's artifact-build path (the same path Phase 1's tasks used), then confirm the embedded bundle the web crate references was refreshed. If the artifact is already rebuilt as the last step of Phase 1, skip to Step 2.

- [ ] **Step 2: Serve the web app.** Run the project's dev-serve command for the `web` crate (e.g. `cargo leptos watch` from `web/`, per the repo's run instructions) and open the served URL in Chrome.

- [ ] **Step 3: Verify the empty state.** With no region selected (initial load), confirm the detail panel shows: the label "World"; the statistic name ("Total fertility rate"); the active year (the bundle's reference year); the world value formatted to two decimals; and "Source: World Bank WDI". Confirm the panel is not blank.

- [ ] **Step 4: Verify selection swaps to the region.** Click a country. Confirm the panel replaces the World figure with that region's name and value, and that the World figure does not linger.

- [ ] **Step 5: Verify deselection returns to World.** Click empty ocean (deselect). Confirm the panel returns to the World figure.

- [ ] **Step 6: Verify the year scrubber updates the World figure.** With no region selected, drag the year scrubber. Confirm the World value and year in the panel update per year, confirming the World default tracks the scrubber (it is republished via `RepublishedViews`).

- [ ] **Step 7: Check the console.** Confirm no `unrecognized data source` warning and no panic in the DevTools console during the above.

- [ ] **Step 8: No commit** (verification only). If any step fails, treat it as a bug in Tasks 36-38 and fix under `superpowers:systematic-debugging` before proceeding.

---

## Open questions / risks

- Phase 0 dependency on `SelectionView.iso3` and `resolve_selection_view`. The current code (`web/src/map/canvas/driver.rs`) still has `SelectionView { iso3, ... }`, `RegionHit.iso3`, `resolve_selection_view(&self, iso3, name_en)` called as `self.resolve_selection_view(&region_hit.iso3, ...)`, `republish` cloning `selection.iso3`, and the `select_region` log line printing `iso3=`. My Task 37 edits (the `republish` rewrite keyed off `self.frame_state.selected_region`, the `resolve_selection_view` call using `region_hit.region_code.0`, and the `SelectionView` destructure without `iso3` in Task 38) all assume Phase 0 has already dropped `SelectionView.iso3`/`RegionHit.iso3` and switched `resolve_selection_view` to take `region_code`. If Phase 0 lands differently (for example, it keeps `RegionHit.iso3` for a genuine display need, contrary to the design's "audit found it is key-only"), Tasks 37-38 need their `region_hit.region_code.0` / destructure adjusted. This is the single largest coupling; confirm Phase 0's final `SelectionView`/`RegionHit`/`resolve_selection_view` shapes before starting Task 37.

- No shared `world` region-code constant exists. There is no `WORLD_REGION_CODE` in `shared` or `ingestion` today; Phase 1 seeds the region by migration and special-cases `"WLD"` in the WDI adapter (mapping to canonical `region.code = "world"`), but nothing defines the literal `"world"` as a reusable constant. Task 37 introduces `const WORLD_REGION_CODE: &str = "world"` in the web driver because the web client is the only consumer of that literal as a shard key. If Phase 1 already introduced a `shared`-level constant for the world region code, Task 37 should import and reuse it instead of redefining the string (per the "reuse constants, no magic restating" convention). Verify Phase 1's output first; if it added such a constant, replace the `const` in Task 37 Step 1 with a `use` of it.

- `t_string!` macro name. Task 38 needs the "World" label as a value in one code path. The `AnyView` form in Task 38 Step 2 sidesteps the macro-name uncertainty entirely and is the recommended default; the `t_string!` form is listed only as the alternative. Confirm via the Step 2 grep before committing.

- `ssr` build does not compile `driver.rs`. It is `#[cfg(feature = "hydrate")]`, so all driver changes (Tasks 37) are only type-checked by the wasm/hydrate `cargo check`. Always run the hydrate check for driver edits; the ssr check alone will pass even if `driver.rs` is broken. `canvas.rs`, `map.rs`, and `detail_panel.rs` are checked by both.

## Deviations from this plan (Phase 2)

- Task 36's unused `_global` context read was not landed as its own commit. `GlobalView`, the `MapView` context, and the driver wiring shipped together because the intermediate unused binding would not have compiled independently of Task 37's `DriverSignals` field once canvas.rs was wired.
- Phase 0 left `region_code` on `SelectionView`. `republish` still re-resolves from `(selection.region_code, selection.name_en)` rather than the plan's `frame_state.selected_region` match. `resolve_selection_view` still writes `region_code`; the plan snippet omitted that field.
- Task 38 uses an `AnyView` region label (`name_en.into_any()` / `t!(i18n, detail.world).into_any()`), the form this plan recommends when `t_string!` is unnecessary.
- No `shared`-level world region-code constant exists after Phase 1 (only `ingestion`'s WDI adapter const). Task 37 defines `WORLD_REGION_CODE` in the web driver as planned.
- Browser verification (2026-08-14): empty state renders "World" / TFR / 2024. The panel shows `detail.no_data` because the live `eafora` DB has a `world` region with zero `statistic_value` rows (the WDI re-ingest that stores `WLD` has not been run). Selecting Algeria replaced the World figure with that region's value and source; clicking empty ocean returned the World figure. The year scrubber's range is a single year (2024), so scrub republish of the World figure was not exercised. Console had only the pre-existing favicon 404.
