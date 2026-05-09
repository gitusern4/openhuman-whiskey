use anyhow::{Context, Result};

fn main() -> Result<()> {
    let err = anyhow::anyhow!("inner").context("outer");
    println!("to_string: {}", err.to_string());
    println!("Display: {}");
    Ok(())
}
