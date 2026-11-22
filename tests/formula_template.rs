//! Guards the Homebrew formula template that `make render-homebrew-formula` fills in.
//!
//! The tap's CI runs `brew audit` on the rendered formula, and Homebrew now rejects the
//! deprecated `post_install` hook ("Formulae must use `post_install_steps`"), which blocked
//! the 0.13.0 tap PR. Post-install guidance belongs in `caveats`.

use std::path::Path;

fn template() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/homebrew/rusty-jack.formula.in");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn test_formula_template_has_no_deprecated_post_install() {
    let template = template();
    assert!(
        !template
            .lines()
            .any(|l| l.trim_start().starts_with("def post_install")),
        "Homebrew rejects `post_install`; put guidance in `caveats` instead"
    );
}

#[test]
fn test_formula_template_keeps_render_placeholders_and_guidance() {
    let template = template();
    for placeholder in ["@ARCHIVE_URL@", "@ARCHIVE_SHA256@", "@GIT_COMMIT@"] {
        assert!(template.contains(placeholder), "missing {placeholder}");
    }
    let caveats = template
        .split("def caveats")
        .nth(1)
        .expect("template defines caveats");
    for needle in [
        "rusty-jack install",
        "rusty-jack upgrade --force",
        "rusty-jack doctor",
    ] {
        assert!(
            caveats.contains(needle),
            "caveats should mention `{needle}`"
        );
    }
    assert!(
        template.contains("generate_completions_from_executable"),
        "completions should be installed"
    );
}
