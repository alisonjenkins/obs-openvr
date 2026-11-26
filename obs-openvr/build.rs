extern crate cc;
extern crate build_profile;
extern crate pkg_config;
extern crate thiserror;

use build_profile::{
    BuildProfile,
    has_feature,
};

use std::{
    error::Error,
    env,
    iter,
    path::{
        Path,
        PathBuf,
    },
    ffi::OsStr,
    str::FromStr,
};

#[derive(Debug, thiserror::Error)]
#[error("Unknown variant for {container_name}: {variant_name}")]
pub struct UnknownVariantError {
    container_name: &'static str,
    variant_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFamily {
    Unix,
    Windows,
    Wasm,
}

impl TargetFamily {
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        let s = env::var("CARGO_CFG_TARGET_FAMILY")?;
        s.parse()
            .map_err(|e| Box::new(e) as _)
    }
}

impl FromStr for TargetFamily {
    type Err = UnknownVariantError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unix" => Ok(TargetFamily::Unix),
            "windows" => Ok(TargetFamily::Windows),
            "wasm" => Ok(TargetFamily::Wasm),
            _ => Err(UnknownVariantError {
                container_name: "TargetFamily",
                variant_name: s.to_owned(),
            }),
        }
    }
}

fn add_link_library(name: &str, kind: Option<&str>) {
    if let Some(kind) = kind {
        println!("cargo:rustc-link-lib={}={}", kind, name);
    } else {
        println!("cargo:rustc-link-lib={}", name);
    }
}

// fn add_sources<'a, It, HIt>(build: &mut cc::Build, sources: It, headers: HIt) where
//     It: Iterator,
//     HIt: Iterator,
// {
//     sources.for_each(|source| {
//         build.file(source);
//         println!("cargo:rerun-if-changed={}", source);
//     });
//     headers.for_each(|header| {
//         println!("cargo:rerun-if-changed={}", header);
//     });
// }

fn main() {
    let profile = BuildProfile::current_or_default();
    let target_family = TargetFamily::from_env()
        .expect("failed to get target family");
    if has_feature("mirror-source") {
        if target_family != TargetFamily::Unix {
            panic!("the mirror source is only supported on Linux");
        }
        let mut gl_copy_build = cc::Build::new();
        if profile.is_debug() {
            gl_copy_build.define("DEBUG", Some("1"));
        }
        gl_copy_build
            .flag("-std=c11")
            .flag("-fPIC")
            .include("src")
            .file("src/gl-copy.c")
            .compile("libobs-openvr-gl-copy.a");
        println!("cargo:rerun-if-changed=src/gl-copy.c");
        println!("cargo:rerun-if-changed=src/gl-copy.h");
        add_link_library("dl", None);
    }
}
