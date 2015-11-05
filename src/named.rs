
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::ops::{Deref, DerefMut};
use std::error;
use std::fmt;
use std::env;
use std;

use super::imp;
use super::util;

/// A named temporary file.
///
/// This variant is *NOT* secure/reliable in the presence of a pathological temporary file cleaner.
///
/// NamedTempFiles are deleted on drop. As rust doesn't guarantee that a struct will ever be
/// dropped, these temporary files will not be deleted on abort, resource leak, early exit, etc.
///
/// Please use TempFile unless you absolutely need a named file.
///
pub struct NamedTempFile(Option<NamedTempFileInner>);

struct NamedTempFileInner {
    file: File,
    path: PathBuf,
}

impl fmt::Debug for NamedTempFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NamedTempFile({:?})", self.0.as_ref().unwrap().path)
    }
}

impl Deref for NamedTempFile {
    type Target = File;
    #[inline]
    fn deref(&self) -> &File {
        &self.inner().file
    }
}

impl DerefMut for NamedTempFile {
    #[inline]
    fn deref_mut(&mut self) -> &mut File {
        &mut self.inner_mut().file
    }
}

/// Error returned when persisting a temporary file fails
#[derive(Debug)]
pub struct PersistError {
    /// The underlying IO error.
    pub error: io::Error,
    /// The temporary file that couldn't be persisted.
    pub file: NamedTempFile,
}

impl From<PersistError> for io::Error {
    #[inline]
    fn from(error: PersistError) -> io::Error {
        error.error
    }
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "failed to persist temporary file: {}", self.error)
    }
}

impl error::Error for PersistError {
    fn description(&self) -> &str {
        "failed to persist temporary file"
    }
    fn cause(&self) -> Option<&error::Error> {
        Some(&self.error)
    }
}

impl NamedTempFile {
    #[inline]
    fn inner(&self) -> &NamedTempFileInner {
        self.0.as_ref().unwrap()
    }

    #[inline]
    fn inner_mut(&mut self) -> &mut NamedTempFileInner {
        self.0.as_mut().unwrap()
    }

    /// Create a new temporary file.
    ///
    /// *SECURITY WARNING:* This will create a temporary file in the default temporary file
    /// directory (platform dependent). These directories are often patrolled by temporary file
    /// cleaners so only use this method if you're *positive* that the temporary file cleaner won't
    /// delete your file.
    ///
    /// Reasons to use this method:
    ///   1. The file has a short lifetime and your temporary file cleaner is sane (doesn't delete
    ///      recently accessed files).
    ///   2. You trust every user on your system (i.e. you are the only user).
    ///   3. You have disabled your system's temporary file cleaner or verified that your system
    ///      doesn't have a temporary file cleaner.
    ///
    /// Reasons not to use this method:
    ///   1. You'll fix it later. No you won't.
    ///   2. You don't care about the security of the temporary file. If none of the "reasons to
    ///      use this method" apply, referring to a temporary file by name may allow an attacker
    ///      to create/overwrite your non-temporary files. There are exceptions but if you don't
    ///      already know them, don't use this method.
    pub fn new() -> io::Result<NamedTempFile> {
        Self::new_in(&env::temp_dir())
    }

    /// Create a new temporary file in the specified directory.
    pub fn new_in<P: AsRef<Path>>(dir: P) -> io::Result<NamedTempFile> {
        Self::template_in(dir, &util::Template::new(::NUM_RAND_CHARS, ".", ""))
    }

    /// Create a new temporary file with file name template
    /// `template` must not any contain '/'s and must contain a sequence of "X".
    /// The sequence of "X" should be longer than 5
    ///
    /// # Examples
    /// ```
    /// use tempfile::NamedTempFile;
    /// 
    /// let template = "hogehogeXXXXXX.rs".parse().unwrap();
    /// let named_temp_file = NamedTempFile::template(&template).unwrap();
    /// println!("{}", named_temp_file.path().display());    //Something like "/tmp/hogehoge65R8Y.rs" 
    /// ```
    pub fn template(template: &util::Template) -> io::Result<NamedTempFile> {
        Self::template_in(&env::temp_dir(), template)
    }

    /// Create a new temporary file from the template in the specified directory.
    pub fn template_in<P: AsRef<Path>>(dir: P, template: &util::Template) -> io::Result<NamedTempFile> {
        for _ in 0..::NUM_RETRIES {
            let path = dir.as_ref().join(&util::tmpname(template));
            return match imp::create_named(&path) {
                Ok(file) => Ok(NamedTempFile(Some(NamedTempFileInner { path: path, file: file, }))),
                Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists,
                           "too many temporary directories already exist"))
        
    }

    /// Get the temporary file's path.
    ///
    /// *SECURITY WARNING:* Only use this method if you're positive that a temporary file cleaner
    /// won't have deleted your file. Otherwise, the path returned by this method may refer to an
    /// attacker controlled file.
    #[inline]
    pub fn path(&self) -> &Path {
        &self.inner().path
    }

    /// Close and remove the temporary file.
    ///
    /// Use this if you want to detect errors in deleting the file.
    pub fn close(mut self) -> io::Result<()> {
        let NamedTempFileInner { path, file } = self.0.take().unwrap();
        drop(file);
        fs::remove_file(path)
    }

    /// Persist the temporary file at the target path.
    ///
    /// If a file exists at the target path, persist will atomically replace it. If this method
    /// fails, it will return `self` in the resulting PersistError.
    ///
    /// Note: Temporary files cannot be persisted across filesystems.
    ///
    /// *SECURITY WARNING:* Only use this method if you're positive that a temporary file cleaner
    /// won't have deleted your file. Otherwise, you might end up persisting an attacker controlled
    /// file.
    pub fn persist<P: AsRef<Path>>(mut self, new_path: P) -> Result<File, PersistError> {
        match imp::persist(&self.inner().path, new_path.as_ref()) {
            Ok(_) => Ok(self.0.take().unwrap().file),
            Err(e) => Err(PersistError { file: self, error: e }),
        }
    }
}

impl Drop for NamedTempFile {
    fn drop(&mut self) {
        if let Some(NamedTempFileInner { file, path }) = self.0.take() {
            drop(file);
            let _ = fs::remove_file(path);
        }
    }
}

impl Read for NamedTempFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (**self).read(buf)
    }
}

impl Write for NamedTempFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (**self).write(buf)
    }
    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        (**self).flush()
    }
}

impl Seek for NamedTempFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        (**self).seek(pos)
    }
}

#[cfg(unix)]
impl std::os::unix::io::AsRawFd for NamedTempFile {
    #[inline]
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        (**self).as_raw_fd()
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for NamedTempFile {
    #[inline]
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        (**self).as_raw_handle()
    }
}
