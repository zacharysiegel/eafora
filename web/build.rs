use std::error::Error;
use std::path::PathBuf;

use leptos_i18n_build::{Config, TranslationsInfos};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
    /* Read through option_env! in live_resolve, which cargo would not otherwise treat as an input, so a
       switch between a local watch and a deploy build would keep the previous repository base. */
    println!("cargo::rerun-if-env-changed=EAFORA_REPOSITORY_BASE_URL");

    let i18n_mod_directory: PathBuf = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("i18n");

    let config: Config = Config::new("en")?;

    let translations_infos: TranslationsInfos = TranslationsInfos::parse(config)?;
    translations_infos.emit_diagnostics();
    translations_infos.rerun_if_locales_changed();
    translations_infos.generate_i18n_module(i18n_mod_directory)?;

    Ok(())
}
