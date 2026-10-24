//! The parts of the `satkeep` command-line tool that other code can use: the
//! wallet folder (`savings.json` and BDK's database) and node access. The
//! binary is in `main.rs`; the fuzz targets use `wallet_dir::Config`.

pub mod node;
pub mod wallet_dir;
