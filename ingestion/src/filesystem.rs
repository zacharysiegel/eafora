use std::path::Path;

use crate::error::AppError;

pub fn filename_of(path: &Path) -> Result<&str, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::from(format!("path missing filename component: {:?}", path)))
}
