use std::fs;
use std::path::Path;
use std::io;

pub fn make_dir_for_file(file_path: &str, creator: &str) -> Result<(), io::Error> {
    let file_name = file_path;
    let dir = Path::new(file_name).parent().unwrap();
    match fs::create_dir_all(dir) {
        Ok(_) => Ok(()),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, format!("could not create dir for {}: {}", creator, err))),
    }
}
