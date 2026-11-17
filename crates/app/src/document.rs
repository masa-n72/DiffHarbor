//! 画面側で扱う 1 ファイル。diff-core の `TextFile` にパスと保存状態を足したもの。

use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use anyhow::Result;
use diff_core::{Encoding, TextFile};

#[derive(Clone)]
pub struct Document {
    pub path: PathBuf,
    pub file: TextFile,
    /// 未保存の変更があるか。履歴の版と `saved_rev` から求めた値を入れておく。
    pub dirty: bool,
    /// 最後に保存した（または読み込んだ）時点の版。
    pub saved_rev: u64,
}

impl Document {
    /// 読み込む。`force` があればその文字コードで読む。
    pub fn load(path: &Path, force: Option<&'static Encoding>) -> Result<Self> {
        Ok(Self { path: path.to_path_buf(), file: diff_core::load(path, force)?, dirty: false, saved_rev: 0 })
    }

    /// 現在のパスへ書き込む。保存状態の更新は呼び出し側で行う。
    pub fn write(&self) -> Result<()> {
        diff_core::save(&self.path, &self.file)
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
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
