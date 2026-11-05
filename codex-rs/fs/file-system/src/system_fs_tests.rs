use super::StdSystemFs;
use super::SystemFs;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn write_and_read_roundtrip() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("file.txt");

    StdSystemFs
        .write(&path, b"hello world")
        .expect("write file");

    assert_eq!(
        StdSystemFs.read_text(&path).expect("read file"),
        "hello world"
    );
    assert_eq!(
        StdSystemFs.read_bytes(&path).expect("read file"),
        b"hello world"
    );
}

#[cfg(unix)]
#[test]
fn write_private_sets_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("nested").join("secret.txt");

    StdSystemFs
        .write_private(&path, b"secret")
        .expect("write private file");

    let mode = std::fs::metadata(&path)
        .expect("read metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn rename_moves_file() {
    let dir = TempDir::new().expect("create temp dir");
    let from = dir.path().join("from.txt");
    let to = dir.path().join("to.txt");

    StdSystemFs.write(&from, b"data").expect("write file");
    StdSystemFs.rename(&from, &to).expect("rename file");

    assert!(!from.exists());
    assert_eq!(StdSystemFs.read_text(&to).expect("read file"), "data");
}

#[test]
fn list_dir_returns_entries() {
    let dir = TempDir::new().expect("create temp dir");
    StdSystemFs
        .write(&dir.path().join("a.txt"), b"a")
        .expect("write file");
    StdSystemFs
        .write(&dir.path().join("b.txt"), b"b")
        .expect("write file");

    let mut entries = StdSystemFs.list_dir(dir.path()).expect("list directory");
    entries.sort();

    let names: Vec<String> = entries
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    assert_eq!(names, vec!["a.txt".to_string(), "b.txt".to_string()]);
}

#[test]
fn create_dir_all_creates_nested_directories() {
    let dir = TempDir::new().expect("create temp dir");
    let nested = dir.path().join("a").join("b").join("c");

    StdSystemFs
        .create_dir_all(&nested)
        .expect("create nested directories");

    assert!(nested.is_dir());
}
