//! The `github` plugin through a stand-in `gh` that replays replies
//! captured from GitHub (see `fixtures/README.md`), and once through the
//! real `gh`, read-only.

mod forge;
mod live;
mod stand_in;
mod tracker;
