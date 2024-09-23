use std::fs;
use std::path::Path;
use tempfile::tempdir;
use anyhow::Result;

fn make_dir_for_file(file_path: &str, _test: &str) -> Result<()> {
    if let Some(parent) = Path::new(file_path).parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[test]
fn test_make_dir_for_file_happy_path() -> Result<()> {
    let temp_dir = tempdir()?;
    let file_path = temp_dir.path().join("testDir/testFile.test");
    make_dir_for_file(file_path.to_str().unwrap(), "test")?;
    
    let f = fs::File::create(&file_path)?;
    drop(f);
    Ok(())
}

#[test]
fn test_make_dir_for_file_negative() -> Result<()> {
    let temp_dir = tempdir()?;
    let file_path = temp_dir.path().join("testFile.test");
    let f = fs::File::create(&file_path)?;
    drop(f);

    let error_path = file_path.join("error");
    let result = make_dir_for_file(error_path.to_str().unwrap(), "test");
    assert!(result.is_err(), "could not create dir for test: mkdir {} : not a directory", error_path.display());
    Ok(())
}
