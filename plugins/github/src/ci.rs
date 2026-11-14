//! The workflow `atelier init` writes: it calls atelier's reusable
//! `gates.yml`, pinned to the commit of the release that wrote it.

use atelier_core::CiFile;

/// Where GitHub reads the workflow from.
pub const WORKFLOW: &str = ".github/workflows/atelier.yml";

/// `.github/workflows/atelier.yml` for `release`, built from `commit`,
/// run on pull requests and on pushes to `base`.
#[must_use]
pub fn workflow(release: &str, commit: &str, base: &str) -> CiFile {
    let text = format!(
        "# Written by atelier init: the project's gates, each its own check.
name: atelier

on:
  pull_request:
  push:
    branches: [{base}]

permissions:
  contents: read

jobs:
  gates:
    uses: wrkhse/atelier/.github/workflows/gates.yml@{commit} # v{release}
    with:
      atelier: {release}
"
    );

    CiFile {
        path: WORKFLOW.to_owned(),
        text,
    }
}
