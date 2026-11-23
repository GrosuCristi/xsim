//! Embeds the application icon (D20, `assets/xsim.ico`) in the Windows
//! executable with the `embed-resource` crate. GPUI loads icon resource 1
//! for its windows, so the icon shows on the window, in the taskbar and on
//! `xsim.exe` itself.
//!
//! The icon is cosmetic: without a resource compiler (`rc.exe` from the
//! Windows SDK, or `llvm-rc`/`windres` when cross-compiling) the build
//! warns and goes on, and the executable keeps the default icon.
//! `assets/make_logo.py` regenerates the icon from the owner's logo.

use embed_resource::{CompilationResult, ParamsIncludeDirs};
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/xsim.rc");
    println!("cargo::rerun-if-changed=assets/xsim.ico");
    // The resource compiler embed-resource uses when one is named; setting
    // one after a build without the icon embeds it on the next build.
    let target = std::env::var("TARGET").unwrap_or_default();
    for variable in [
        format!("RC_{target}"),
        format!("RC_{}", target.replace('-', "_")),
        "RC".to_string(),
    ] {
        println!("cargo::rerun-if-env-changed={variable}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_icon();
    }
}

fn embed_icon() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let result = embed_resource::compile_for(
        assets.join("xsim.rc"),
        ["xsim"],
        ParamsIncludeDirs([assets.as_os_str()]),
    );
    match result {
        CompilationResult::Ok | CompilationResult::NotWindows => {}
        problem @ (CompilationResult::NotAttempted(_) | CompilationResult::Failed(_)) => {
            // A warning is one line; compiler output can have several.
            let problem = problem.to_string().replace(['\r', '\n'], " ");
            println!(
                "cargo::warning=xsim.exe is built without its icon ({problem}); \
                 to embed it, set RC to a resource compiler, or install the \
                 Windows SDK's and run `cargo clean -p xsim-desktop`"
            );
        }
    }
}
