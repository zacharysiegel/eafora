use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHfdPublication {
    pub revision_label: String,
    pub last_modified: NaiveDate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedHfdStatisticValue {
    pub hfd_code: String,
    pub cohort_year: i32,
    pub value: Option<f64>,
}
