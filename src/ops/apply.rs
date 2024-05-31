use std::{
    io::Write,
    path::{Path, PathBuf},
};

use log::error;
use termcolor::{ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::{
    fixture::{Fixture, FixtureType},
    repo, template,
};

pub fn apply_fixtures(
    fixtures: Vec<Fixture>,
    revert: bool,
    no_backup: bool,
) -> std::io::Result<()> {
    let backup_dir = dirs::state_dir().unwrap().join("spaceconf");
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    for fixture in fixtures {
        if fixture.skip() {
            continue;
        }

        match fixture.fixture_type {
            FixtureType::Files(setup) => {
                for file in setup.files {
                    let Some(src) = file.src.resolve() else {
                        continue;
                    };
                    let Some(dest) = file.dest.resolve() else {
                        continue;
                    };

                    stdout.set_color(ColorSpec::new().set_fg(Some(termcolor::Color::White)))?;
                    if revert {
                        if let Err(e) = restore_file(&backup_dir, &dest, setup.root) {
                            eprintln!("Failed to restore {:?}: {}", dest, e);
                            return Err(e);
                        }
                    } else {
                        let output = if file.raw {
                            std::fs::read_to_string(&src).inspect_err(|_| {
                                error!("failed to read source file: {}", &src.to_string_lossy())
                            })?
                        } else {
                            let input = std::fs::read_to_string(&src).inspect_err(|_| {
                                error!("failed to read source file: {}", &src.to_string_lossy())
                            })?;
                            template::render(&input, &setup.secrets).unwrap()
                        };

                        if check_content(&output, &dest) {
                            stdout.set_color(
                                ColorSpec::new().set_fg(Some(termcolor::Color::Green)),
                            )?;
                            writeln!(&mut stdout, "{} is up to date", dest.to_string_lossy())
                                .unwrap();
                            stdout.set_color(
                                ColorSpec::new().set_fg(Some(termcolor::Color::White)),
                            )?;
                            continue;
                        }

                        if !no_backup {
                            if !backup_dir.exists() {
                                std::fs::create_dir_all(&backup_dir).inspect_err(|_| {
                                    error!(
                                        "failed to create parent directory(s): {}",
                                        &backup_dir.to_string_lossy()
                                    )
                                })?;
                            }
                            backup_file(&backup_dir, &dest);
                        }

                        if setup.root {
                            write_root(&dest, &output)?;
                        } else {
                            std::fs::write(&dest, output).inspect_err(|_| {
                                error!(
                                    "failed to read destination file: {}",
                                    &dest.to_string_lossy()
                                )
                            })?;
                        }
                        println!("Applying {:?}", dest);
                    }
                }
            }
            FixtureType::Repository(setup) => {
                repo::apply(setup.clone());
            }
        }
    }

    Ok(())
}

fn check_content(content: &str, output: &PathBuf) -> bool {
    if !output.exists() {
        return false;
    }

    let existing_content = std::fs::read_to_string(output).unwrap();

    content == existing_content
}

fn backup_file(backup_dir: &Path, file: &PathBuf) {
    if !file.exists() {
        return;
    }

    let backup_file = get_backup_filename(backup_dir, file);
    std::fs::create_dir_all(backup_file.parent().unwrap()).unwrap();
    std::fs::copy(file, backup_file).unwrap();
}

fn restore_file(backup_dir: &Path, file: &PathBuf, root: bool) -> std::io::Result<()> {
    let backup_file = get_backup_filename(backup_dir, file);
    if !backup_file.exists() {
        eprintln!("Backup file does not exist for {:?}", file);
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Backup file does not exist",
        ));
    }

    if root {
        write_root(file, &std::fs::read_to_string(backup_file).unwrap())?;
    } else {
        std::fs::copy(backup_file, file).unwrap();
    }
    Ok(())
}

fn get_backup_filename(backup_dir: &Path, file: &Path) -> PathBuf {
    backup_dir.join(file.strip_prefix("/").unwrap())
}

fn write_root(file: &PathBuf, content: &str) -> std::io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("Root fixture is currently only supported on Linux");
        std::process::exit(1);
    }

    let temp_file = PathBuf::from(format!("/tmp/spaceconf-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp_file, content).inspect_err(|_| {
        error!(
            "failed to write temporary file: {}",
            &temp_file.to_string_lossy()
        )
    })?;
    if !file.parent().unwrap().exists() {
        std::process::Command::new("sudo")
            .arg("mkdir")
            .arg("-p")
            .arg(file.parent().unwrap())
            .status()
            .unwrap();
    }
    std::process::Command::new("sudo")
        .arg("cp")
        .arg(&temp_file)
        .arg(file)
        .status()
        .unwrap();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::fixture::{self, FileDefinition, Fixture};

    #[test]
    fn test_apply_files_fixture() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let file_content = "Hello, World!";

        let source_file = test_dir.path().join("source.conf");
        let dest_file = test_dir.path().join("dest.conf");

        std::fs::write(&source_file, file_content).unwrap();

        assert!(!dest_file.exists());

        let fixture = Fixture {
            name: "test-fixture".into(),
            include_for: None,
            exclude_for: None,
            fixture_type: FixtureType::Files(fixture::FilesSetup {
                files: vec![fixture::File {
                    src: FileDefinition::Single(source_file.clone()),
                    dest: FileDefinition::Single(dest_file.clone()),
                    raw: true,
                    optional: false,
                }],
                root: false,
                secrets: Default::default(),
            }),
        };

        apply_fixtures(vec![fixture], false, true).unwrap();

        assert!(dest_file.exists());

        let dest_content = std::fs::read_to_string(&dest_file).unwrap();

        assert_eq!(dest_content, file_content);
    }

    #[test]
    fn test_apply_excluded_fixture() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let file_content = "Hello, World!";

        let source_file = test_dir.path().join("source.conf");
        let dest_file = test_dir.path().join("dest.conf");

        std::fs::write(&source_file, file_content).unwrap();

        assert!(!dest_file.exists());

        let fixture = Fixture {
            name: "test-fixture".into(),
            include_for: None,
            exclude_for: Some(vec![std::env::consts::OS.into()]),
            fixture_type: FixtureType::Files(fixture::FilesSetup {
                files: vec![fixture::File {
                    src: FileDefinition::Single(source_file.clone()),
                    dest: FileDefinition::Single(dest_file.clone()),
                    raw: true,
                    optional: false,
                }],
                root: false,
                secrets: Default::default(),
            }),
        };

        apply_fixtures(vec![fixture], false, true).unwrap();

        assert!(!dest_file.exists());
    }

    #[test]
    fn test_apply_unresolved_source() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let file_content = "Hello, World!";

        let source_file = test_dir.path().join("source.conf");
        let dest_file = test_dir.path().join("dest.conf");

        std::fs::write(&source_file, file_content).unwrap();

        assert!(!dest_file.exists());

        let fixture = Fixture {
            name: "test-fixture".into(),
            include_for: None,
            exclude_for: None,
            fixture_type: FixtureType::Files(fixture::FilesSetup {
                files: vec![fixture::File {
                    src: FileDefinition::Multiple(HashMap::from_iter(vec![(
                        "nonexistent".into(),
                        source_file.clone(),
                    )])),
                    dest: FileDefinition::Single(dest_file.clone()),
                    raw: true,
                    optional: true,
                }],
                root: false,
                secrets: Default::default(),
            }),
        };

        apply_fixtures(vec![fixture], false, true).unwrap();

        assert!(!dest_file.exists());
    }

    #[test]
    fn test_apply_unresolved_destination() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let file_content = "Hello, World!";

        let source_file = test_dir.path().join("source.conf");
        let dest_file = test_dir.path().join("dest.conf");

        std::fs::write(&source_file, file_content).unwrap();

        assert!(!dest_file.exists());

        let fixture = Fixture {
            name: "test-fixture".into(),
            include_for: None,
            exclude_for: None,
            fixture_type: FixtureType::Files(fixture::FilesSetup {
                files: vec![fixture::File {
                    src: FileDefinition::Single(source_file.clone()),
                    dest: FileDefinition::Multiple(HashMap::from_iter(vec![(
                        "nonexistent".into(),
                        dest_file.clone(),
                    )])),
                    raw: true,
                    optional: true,
                }],
                root: false,
                secrets: Default::default(),
            }),
        };

        apply_fixtures(vec![fixture], false, true).unwrap();

        assert!(!dest_file.exists());
    }

    #[test]
    fn test_backup_file() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let backup_dir = test_dir.path().join("backup");
        std::fs::create_dir(&backup_dir).unwrap();

        let file = test_dir.path().join("file.txt");
        std::fs::write(&file, "Hello, World!").unwrap();

        let backup_filename = backup_dir.join(file.strip_prefix("/").unwrap());

        assert!(!backup_filename.exists());

        backup_file(&backup_dir, &file);

        assert!(backup_filename.exists());
    }

    #[test]
    fn test_restore_file() {
        let test_dir = tempfile::tempdir().expect("Failed to create temporary directory");

        let backup_dir = test_dir.path().join("backup");
        std::fs::create_dir(&backup_dir).unwrap();

        let file = test_dir.path().join("file.txt");
        std::fs::write(file.clone(), "Hello, World!").unwrap();

        let backup_filename = backup_dir.join(file.strip_prefix("/").unwrap());
        std::fs::create_dir_all(backup_filename.parent().unwrap()).unwrap();
        std::fs::write(backup_filename, "Hello, Backup!").unwrap();

        let restored_file = test_dir.path().join("file.txt");

        let pre_restore_content = std::fs::read_to_string(&restored_file).unwrap();

        assert_eq!(pre_restore_content, "Hello, World!");

        restore_file(&backup_dir, &restored_file, false).unwrap();

        assert!(restored_file.exists());

        let restored_content = std::fs::read_to_string(&restored_file).unwrap();

        assert_eq!(restored_content, "Hello, Backup!");
    }
}
