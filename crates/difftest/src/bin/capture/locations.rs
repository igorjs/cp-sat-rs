//! Where committed fixture and input files live in the repository.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Where committed fixture files live: `crates/*/tests/fixtures/*.txt`, as
/// (directory of the crates, directory in a crate, file extension).
const FIXTURE_LOCATION: (&str, &str, &str) = ("crates", "tests/fixtures", "txt");

/// Where input files live inside a crate: a folder in the fixture folder and the
/// file extension, as (folder, extension).
#[cfg(test)]
const INPUT_LOCATION: (&str, &str) = ("inputs", "in");

fn directory_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .collect();
    paths.sort();
    paths
}

pub fn find_fixtures(root: &Path) -> Vec<PathBuf> {
    let (crates_dir, fixture_dir, extension) = FIXTURE_LOCATION;
    directory_files(&root.join(crates_dir))
        .into_iter()
        .flat_map(|crate_dir| directory_files(&crate_dir.join(fixture_dir)))
        .filter(|path| path.is_file() && path.extension().is_some_and(|found| found == extension))
        .collect()
}

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(test)]
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cpsat-difftest-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_fixtures_reads_every_text_file_under_a_crate_fixture_folder() {
        // Arrange
        let root = temp_dir("find");
        let fixtures = root.join("crates/alpha/tests/fixtures");
        fs::create_dir_all(&fixtures).unwrap();
        fs::create_dir_all(root.join("crates/beta/tests")).unwrap();
        fs::write(fixtures.join("one.txt"), "").unwrap();
        fs::write(fixtures.join("notes.md"), "").unwrap();

        // Act
        let found = find_fixtures(&root);

        // Assert
        assert_eq!(found, vec![fixtures.join("one.txt")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn find_fixtures_ignores_input_files() {
        // Arrange
        let root = temp_dir("inputs");
        let (inputs_dir, extension) = INPUT_LOCATION;
        let inputs = root.join("crates/alpha/tests/fixtures").join(inputs_dir);
        fs::create_dir_all(&inputs).unwrap();
        fs::write(inputs.join(format!("parsed.model.{extension}")), "").unwrap();
        fs::write(inputs.join("parsed.model.txt"), "").unwrap();

        // Act
        let found = find_fixtures(&root);

        // Assert
        assert!(found.is_empty(), "{found:?}");
        fs::remove_dir_all(root).unwrap();
    }
}
