//! 画面側で扱う 1 ファイル。diff-core の `TextFile` にパスと変更フラグを足したもの。

use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use anyhow::Result;
use diff_core::TextFile;

#[derive(Clone)]
pub struct Document {
    pub path: PathBuf,
    pub file: TextFile,
    pub dirty: bool,
}

impl Document {
    pub fn load(path: &Path) -> Result<Self> {
        Ok(Self { path: path.to_path_buf(), file: diff_core::load(path, None)?, dirty: false })
    }

    pub fn save(&mut self) -> Result<()> {
        diff_core::save(&self.path, &self.file)?;
        self.dirty = false;
        Ok(())
    }
}

impl Deref for Document {
    type Target = TextFile;
    fn deref(&self) -> &TextFile {
        &self.file
    }
}

impl DerefMut for Document {
    fn deref_mut(&mut self) -> &mut TextFile {
        &mut self.file
    }
}
