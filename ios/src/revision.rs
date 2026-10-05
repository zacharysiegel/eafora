#[uniffi::export]
pub fn revision() -> String {
    shared::revision::REVISION.to_string()
}
