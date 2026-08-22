use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHfdPublication {
    pub revision_label: String,
    pub last_modified: NaiveDate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedHfdStatisticValue {
    pub hfd_code: String,
    /// The birth cohort in a cohort file, the calendar year in a period file.
    pub period_year: i32,
    pub value: Option<f64>,
}
