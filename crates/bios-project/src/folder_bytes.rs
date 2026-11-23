//! Large and binary files in a folder's drawer, and removing a typed folder.
//!
//! A verification folder keeps the reads it was sent (hundreds of megabytes of gzip, copied in as
//! they arrive, never loaded whole) and every alignment made from them (gzipped). Both live in the
//! folder's own `.bios/`, so they move with the folder and go when it goes.

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::project::{DOT, Error, Project, clean_folder, io};

/// A drawer name: one or two segments of `[A-Za-z0-9._-]`, each starting alphanumeric. Nobody
/// types these; Bios mints them.
fn check(name: &str) -> Result<(), Error> {
    let parts: Vec<&str> = name.split('/').collect();
    let ok = !name.is_empty()
        && parts.len() <= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 160
                && part.starts_with(|c: char| c.is_ascii_alphanumeric())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        });
    if ok { Ok(()) } else { Err(Error::Unwritable(format!("{name:?} is not a name a folder can keep a file under"))) }
}

impl Project {
    fn drawer(&self, folder: &str, name: &str) -> Result<PathBuf, Error> {
        check(name)?;
        let rel = clean_folder(folder)?;
        if rel.as_os_str().is_empty() {
            return Err(Error::BadFolder(folder.to_string()));
        }
        Ok(self.root().join(rel).join(DOT).join(name))
    }

    /// Copies the file at `source` (outside the project) into `folder`'s drawer as `name`, without
    /// loading it, and returns its size. It is copied beside its place first and then moved in, so
    /// a reader never finds half of it. An existing file of that name is replaced.
    pub fn import_folder_file(&self, folder: &str, name: &str, source: &Path) -> Result<u64, Error> {
        let path = self.drawer(folder, name)?;
        let dir = path.parent().ok_or_else(|| Error::BadFolder(folder.to_string()))?;
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        let partial = dir.join(format!(".{}.partial", uuid::Uuid::new_v4().simple()));
        let bytes = fs::copy(source, &partial).map_err(|e| io(source, e))?;
        let _lock = self.lock()?;
        fs::rename(&partial, &path).map_err(|e| {
            let _ = fs::remove_file(&partial);
            io(&path, e)
        })?;
        Ok(bytes)
    }

    /// Writes bytes into `folder`'s drawer as `name`, whole or not at all.
    pub fn write_folder_bytes(&self, folder: &str, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let path = self.drawer(folder, name)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        let _lock = self.lock()?;
        craft_atomic::atomic_write(&path, bytes).map_err(|e| io(&path, e))
    }

    /// The bytes of one file in `folder`'s drawer, or `None` when it is not there.
    pub fn read_folder_bytes(&self, folder: &str, name: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.drawer(folder, name)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&path, e)),
        }
    }

    /// One file in `folder`'s drawer opened for reading, to stream it, or `None` when it is not
    /// there.
    pub fn open_folder_file(&self, folder: &str, name: &str) -> Result<Option<File>, Error> {
        let path = self.drawer(folder, name)?;
        match File::open(&path) {
            Ok(file) => Ok(Some(file)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&path, e)),
        }
    }

    /// Removes a folder that holds no constructs any more, with its drawer. A folder that still
    /// holds a plasmid is refused: nothing can take a construct with it by accident.
    pub fn remove_folder(&self, folder: &str) -> Result<(), Error> {
        let rel = clean_folder(folder)?;
        if rel.as_os_str().is_empty() {
            return Err(Error::BadFolder(folder.to_string()));
        }
        let path = self.root().join(&rel);
        if !path.is_dir() {
            return Err(Error::BadFolder(folder.to_string()));
        }
        let holds = self.list()?.iter().any(|l| l.path.starts_with(&rel));
        if holds {
            return Err(Error::Unwritable(format!("{folder} still holds plasmids; move or delete them first")));
        }
        let _lock = self.lock()?;
        fs::remove_dir_all(&path).map_err(|e| io(&path, e))
    }
}

/// Writes bytes to a file outside any project, such as an alignment's zip a person asked for. An
/// existing file is replaced only with `overwrite`.
pub fn write_outside_bytes(path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let written = if overwrite { craft_atomic::atomic_write(path, bytes) } else { craft_atomic::write_new(path, bytes) };
    written.map_err(|e| if e.kind() == ErrorKind::AlreadyExists { Error::NameTaken(path.to_path_buf()) } else { io(path, e) })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::{Actor, Project, Who};

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(dir.path(), "P").unwrap();
        (dir, project)
    }

    #[test]
    fn a_file_copied_in_reads_back_byte_for_byte_and_moves_with_its_folder() {
        let (dir, project) = project();
        let source = dir.path().join("..reads.fastq.gz");
        std::fs::write(&source, b"\x1f\x8b binary").unwrap();
        project.create_folder("Run 1").unwrap();
        assert_eq!(project.import_folder_file("Run 1", "reads/A1_R1.fastq.gz", &source).unwrap(), 9);
        let mut text = Vec::new();
        project.open_folder_file("Run 1", "reads/A1_R1.fastq.gz").unwrap().unwrap().read_to_end(&mut text).unwrap();
        assert_eq!(text, b"\x1f\x8b binary");
        project.rename_folder("Run 1", "Run 2").unwrap();
        assert!(project.read_folder_bytes("Run 2", "reads/A1_R1.fastq.gz").unwrap().is_some());
        assert!(project.read_folder_bytes("Run 2", "reads/nothing").unwrap().is_none());
    }

    #[test]
    fn names_that_could_leave_the_drawer_are_refused() {
        let (_dir, project) = project();
        project.create_folder("Run").unwrap();
        for bad in ["../x", "a/b/c", ".hidden", "", "a b"] {
            assert!(project.write_folder_bytes("Run", bad, b"x").is_err(), "{bad}");
        }
    }

    #[test]
    fn a_folder_still_holding_a_plasmid_is_not_removed() {
        let (dir, project) = project();
        let who = Who { actor: Actor::Human, source: "test".into(), session: None };
        let source = dir.path().join("map.fasta");
        std::fs::write(&source, ">map\nACGTACGTACGTACGTACGTACGT\n").unwrap();
        let made = project.import(&source, "Run", None, &who).unwrap().construct;
        assert!(project.remove_folder("Run").is_err());
        project.delete(&made.id, &who).unwrap();
        project.write_folder_bytes("Run", "runs/x.json.gz", b"x").unwrap();
        project.remove_folder("Run").unwrap();
        assert!(project.folders().unwrap().is_empty());
    }
}
