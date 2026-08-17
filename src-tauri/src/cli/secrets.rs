//! Secret/keychain commands for the CLI.

use crate::secrets;
use anyhow::Result;

pub fn selftest() -> Result<()> {
    println!("{}", secrets::selftest());
    Ok(())
}
