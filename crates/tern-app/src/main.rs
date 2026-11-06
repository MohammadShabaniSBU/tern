mod paths;

fn main() -> anyhow::Result<()> {
    let data_dir = paths::ensure_data_dir()?;
    println!("data dir: {}", data_dir.display());
    Ok(())
}
