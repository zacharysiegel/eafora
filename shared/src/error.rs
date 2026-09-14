use std::error::Error;

minimer::define_app_error!(pub AppError);

/* The error of any operation a non-browser client awaits across its FFI. UniFFI requires the future it
   returns to be Send, which AppError is not: it boxes a source error under no Send bound. */
minimer::define_app_error_static!(pub AppErrorStatic);

minimer::impl_from_error!(AppError, serde_json::Error);
minimer::impl_from_error!(AppError, flatgeobuf::Error);
minimer::impl_from_error!(AppError, geozero::error::GeozeroError);

#[cfg(not(target_arch = "wasm32"))] // We use a different SQLite library for the Wasm target
minimer::impl_from_error!(AppError, rusqlite::Error);

impl From<AppError> for AppErrorStatic {
    fn from(error: AppError) -> AppErrorStatic {
        AppErrorStatic(minimer::AppErrorStatic::from(error.0))
    }
}

impl From<AppErrorStatic> for AppError {
    fn from(error: AppErrorStatic) -> AppError {
        AppError(minimer::AppError::from(error.0))
    }
}

pub fn render_error_chain(error: &dyn Error) -> String {
    let mut rendered: String = error.to_string();
    let mut next: Option<&dyn Error> = error.source();

    while let Some(source) = next {
        rendered.push_str(" -> ");
        rendered.push_str(&source.to_string());
        next = source.source();
    }

    rendered
}
