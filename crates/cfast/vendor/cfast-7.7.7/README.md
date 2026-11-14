# CFAST 7.7.7 (vendored)

Unmodified Fortran sources of [CFAST](https://github.com/firemodels/cfast),
the Consolidated Model of Fire and Smoke Transport developed by the National
Institute of Standards and Technology (NIST). See `LICENSE.md` for NIST's
notice, which must stay intact.

- Upstream tag: `CFAST-7.7.7`
- Upstream commit: `b43e74e0f43c704582c5782f5446597ce729a07f`
- Copied: `Source/CFAST/*.f90` and `LICENSE.md` only (no manuals, GUI, or
  validation data)

`crates/cfast/build.rs` compiles these files with `gfortran` during
`cargo build`, using the same flags as upstream's `Build/CFAST/makefile`
(`gnu_osx`, `gnu_linux`, or `gnu_win`). The source files themselves are not
changed.

To upgrade, replace `Source/` with the new tag's `Source/CFAST/*.f90`, update
the tag, commit, and `CFAST_GIT_DESCRIBE` in `build.rs`, and bump
`CFAST_SUPPORTED_VERSION` in `src/runner.rs`.
