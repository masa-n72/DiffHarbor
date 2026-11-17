//! テキストファイルの読み書き。文字コード、改行コード、BOM を保持する。

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8};

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
        }
    }
}

/// 行の並びとして保持したテキスト。保存時に元の形式へ戻すための情報を持つ。
#[derive(Clone, Debug)]
pub struct TextFile {
    /// 改行を含まない各行。
    pub lines: Vec<String>,
    pub enc: &'static Encoding,
    /// 保存時に使う改行コード。混在していた場合は最頻のもの。
    pub eol: Eol,
    /// UTF-8 の BOM が付いていたか。
    pub bom: bool,
    pub trailing_newline: bool,
    /// LF と CRLF が混在していたか。
    pub mixed_eol: bool,
    /// 文字コードを内容から推定したか（BOM も UTF-8 としての妥当性もなかった）。
    pub guessed: bool,
    /// 読み込み時に変換できないバイトがあったか。true の間は保存できない。
    pub lossy: bool,
}

impl TextFile {
    /// UTF-8、BOM なしのテキストとして作る。
    pub fn from_text(text: &str) -> Self {
        let (lines, eol, mixed_eol, trailing_newline) = split_lines(text);
        Self { lines, enc: UTF_8, eol, bom: false, trailing_newline, mixed_eol, guessed: false, lossy: false }
    }

    pub fn to_text(&self) -> String {
        let mut s = self.lines.join(self.eol.as_str());
        if self.trailing_newline && !self.lines.is_empty() {
            s.push_str(self.eol.as_str());
        }
        s
    }

    /// 表示用の文字コード名。例: `UTF-8`、`UTF-8 (BOM)`、`Shift_JIS (推定)`。
    pub fn encoding_label(&self) -> String {
        let mut s = self.enc.name().to_string();
        if self.bom {
            s.push_str(" (BOM)");
        }
        if self.guessed {
            s.push_str(" (推定)");
        }
        s
    }

    /// 表示用の改行コード名。例: `LF`、`CRLF (混在)`。
    pub fn eol_label(&self) -> String {
        if self.mixed_eol {
            format!("{} (混在)", self.eol.label())
        } else {
            self.eol.label().to_string()
        }
    }

    /// 元の文字コードのバイト列に戻す。表せない文字があればエラー。
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.lossy {
            bail!("読み込み時に変換できないバイトがあったため、保存できません");
        }
        let text = self.to_text();
        let (bytes, _, unmappable) = self.enc.encode(&text);
        if unmappable {
            let line = self
                .lines
                .iter()
                .position(|l| self.enc.encode(l).2)
                .map(|i| i + 1)
                .unwrap_or(0);
            bail!("{} 行目に {} で表せない文字があります。保存を中止しました", line, self.enc.name());
        }
        let mut out = Vec::with_capacity(bytes.len() + 3);
        if self.bom {
            out.extend_from_slice(UTF8_BOM);
        }
        out.extend_from_slice(&bytes);
        Ok(out)
    }
}

/// バイト列をテキストとして解釈する。`force` があればその文字コードで読む。
pub fn decode(bytes: &[u8], force: Option<&'static Encoding>) -> Result<TextFile> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        bail!("UTF-16 のファイルは未対応です");
    }
    if bytes.contains(&0) {
        bail!("バイナリファイルは比較できません");
    }
    let has_bom = bytes.starts_with(UTF8_BOM);

    let (enc, guessed) = match force {
        Some(e) => (e, false),
        None if has_bom => (UTF_8, false),
        None if std::str::from_utf8(bytes).is_ok() => {
            // ISO-2022-JP は 7 ビットなので UTF-8 としても妥当に見える。
            if looks_like_iso_2022_jp(bytes) {
                bail!("ISO-2022-JP (JIS) のファイルは未対応です");
            }
            (UTF_8, false)
        }
        None => {
            let mut det = EncodingDetector::new(Iso2022JpDetection::Deny);
            det.feed(bytes, true);
            // 日本語の文字コードを優先して推定する。
            (det.guess(Some(b"jp"), Utf8Detection::Deny), true)
        }
    };
    // 書き戻せない文字コード（UTF-16、ISO-2022-JP など）は扱わない。
    if enc.output_encoding() != enc {
        bail!("{} のファイルは未対応です", enc.name());
    }

    let bom = has_bom && enc == UTF_8;
    let body = if bom { &bytes[UTF8_BOM.len()..] } else { bytes };
    let (text, lossy) = enc.decode_without_bom_handling(body);
    let (lines, eol, mixed_eol, trailing_newline) = split_lines(&text);
    Ok(TextFile { lines, enc, eol, bom, trailing_newline, mixed_eol, guessed, lossy })
}

pub fn load(path: &Path, force: Option<&'static Encoding>) -> Result<TextFile> {
    let bytes = fs::read(path).with_context(|| format!("読み込めません: {}", path.display()))?;
    decode(&bytes, force).map_err(|e| anyhow!("{}: {}", e, path.display()))
}

/// 一時ファイルに書いてから置き換える。失敗した場合、元のファイルは変わらない。
pub fn save(path: &Path, file: &TextFile) -> Result<()> {
    let bytes = file.encode()?;
    // シンボリックリンクはリンク先を置き換える。
    let target: PathBuf = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = fs::metadata(&target).ok();
    if let Some(meta) = &existing {
        if meta.permissions().readonly() {
            bail!("読み取り専用のため保存できません: {}", path.display());
        }
    }
    let dir = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = target.file_name().ok_or_else(|| anyhow!("保存先が不正です: {}", path.display()))?;
    let tmp = dir.join(format!(".{}.dhtmp-{}", name.to_string_lossy(), std::process::id()));

    let result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        if let Some(meta) = &existing {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("保存できません: {}", path.display()))
}

fn looks_like_iso_2022_jp(bytes: &[u8]) -> bool {
    bytes
        .windows(3)
        .any(|w| w[0] == 0x1B && matches!((w[1], w[2]), (b'$', b'B') | (b'$', b'@') | (b'(', b'J')))
}

/// 行に分割する。戻り値は (行, 改行コード, 混在, 末尾改行)。
fn split_lines(text: &str) -> (Vec<String>, Eol, bool, bool) {
    let mut pieces: Vec<&str> = text.split('\n').collect();
    // 最後の要素だけは改行で終わっていない。
    let last = pieces.pop().unwrap_or("");
    let (mut crlf, mut lf) = (0usize, 0usize);
    let mut lines = Vec::with_capacity(pieces.len() + 1);
    for piece in pieces {
        match piece.strip_suffix('\r') {
            Some(s) => {
                crlf += 1;
                lines.push(s.to_string());
            }
            None => {
                lf += 1;
                lines.push(piece.to_string());
            }
        }
    }
    if !last.is_empty() {
        lines.push(last.to_string());
    }
    let eol = if crlf > lf { Eol::CrLf } else { Eol::Lf };
    (lines, eol, crlf > 0 && lf > 0, text.ends_with('\n'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{EUC_JP, SHIFT_JIS};

    const JA: &str = "これは日本語のテキストです。\r\n文字コードの自動判定を確認します。\r\n漢字、ひらがな、カタカナ、ＡＢＣ、123 を含みます。\r\n";

    fn tmp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("diff-core-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn roundtrip_preserves_eol_and_trailing_newline() {
        for text in ["a\nb\n", "a\r\nb\r\n", "a\nb", "", "\n", "a\r\nb", "\r\n"] {
            let f = decode(text.as_bytes(), None).unwrap();
            assert_eq!(f.encode().unwrap(), text.as_bytes(), "{text:?}");
            assert!(!f.mixed_eol);
        }
    }

    #[test]
    fn detects_shift_jis_and_roundtrips() {
        let (bytes, _, bad) = SHIFT_JIS.encode(JA);
        assert!(!bad);
        let f = decode(&bytes, None).unwrap();
        assert_eq!(f.enc, SHIFT_JIS);
        assert!(f.guessed && !f.lossy);
        assert_eq!(f.eol, Eol::CrLf);
        assert_eq!(f.lines[0], "これは日本語のテキストです。");
        assert_eq!(f.encode().unwrap(), bytes.as_ref());
    }

    #[test]
    fn detects_euc_jp_and_roundtrips() {
        let (bytes, _, bad) = EUC_JP.encode(JA);
        assert!(!bad);
        let f = decode(&bytes, None).unwrap();
        assert_eq!(f.enc, EUC_JP);
        assert_eq!(f.lines.len(), 3);
        assert_eq!(f.encode().unwrap(), bytes.as_ref());
    }

    #[test]
    fn utf8_is_not_guessed_and_bom_is_kept() {
        let f = decode(JA.as_bytes(), None).unwrap();
        assert_eq!(f.enc, UTF_8);
        assert!(!f.guessed && !f.bom);

        let mut with_bom = UTF8_BOM.to_vec();
        with_bom.extend_from_slice(JA.as_bytes());
        let f = decode(&with_bom, None).unwrap();
        assert!(f.bom);
        assert_eq!(f.lines[0], "これは日本語のテキストです。");
        assert_eq!(f.encode().unwrap(), with_bom);
        assert_eq!(f.encoding_label(), "UTF-8 (BOM)");
    }

    #[test]
    fn rejects_unsupported_inputs() {
        assert!(decode(&[0xFF, 0xFE, b'a', 0], None).is_err(), "UTF-16");
        assert!(decode(b"abc\0def", None).is_err(), "binary");
        assert!(decode(b"\x1b$B$3$s$K$A$O\x1b(B\n", None).is_err(), "ISO-2022-JP");
        assert!(decode(b"abc", Some(encoding_rs::UTF_16LE)).is_err(), "forced UTF-16");
    }

    #[test]
    fn forced_encoding_overrides_detection() {
        let (bytes, _, _) = SHIFT_JIS.encode("短い");
        let f = decode(&bytes, Some(SHIFT_JIS)).unwrap();
        assert_eq!(f.lines, ["短い"]);
        assert!(!f.guessed);
    }

    #[test]
    fn unmappable_characters_abort_encode_with_line_number() {
        let (bytes, _, _) = SHIFT_JIS.encode(JA);
        let mut f = decode(&bytes, None).unwrap();
        f.lines.push("絵文字 😀".to_string());
        let err = f.encode().unwrap_err().to_string();
        assert!(err.contains("4 行目"), "{err}");
    }

    #[test]
    fn invalid_bytes_mark_lossy_and_block_save() {
        // UTF-8 として強制的に読むと Shift_JIS のバイトは変換できない。
        let (bytes, _, _) = SHIFT_JIS.encode(JA);
        let f = decode(&bytes, Some(UTF_8)).unwrap();
        assert!(f.lossy);
        assert!(f.encode().is_err());
    }

    #[test]
    fn mixed_eol_uses_majority() {
        let f = decode(b"a\r\nb\r\nc\nd\r\n", None).unwrap();
        assert!(f.mixed_eol);
        assert_eq!(f.eol, Eol::CrLf);
        assert_eq!(f.lines, ["a", "b", "c", "d"]);
        assert_eq!(f.eol_label(), "CRLF (混在)");
        let f = decode(b"a\nb\nc\r\n", None).unwrap();
        assert_eq!(f.eol, Eol::Lf);
    }

    #[cfg(unix)]
    #[test]
    fn save_replaces_atomically_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir("save");
        let path = dir.join("a.txt");
        fs::write(&path, "old\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        let mut f = load(&path, None).unwrap();
        f.lines = vec!["new".into(), "行".into()];
        save(&path, &f).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n行\n");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        // 一時ファイルが残っていない。
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn save_refuses_readonly_and_failed_encode_leaves_file_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir("readonly");
        let path = dir.join("ro.txt");
        fs::write(&path, "keep\n").unwrap();
        let mut f = load(&path, None).unwrap();
        f.lines[0] = "changed".into();

        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        assert!(save(&path, &f).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let (bytes, _, _) = SHIFT_JIS.encode(JA);
        fs::write(&path, &bytes).unwrap();
        let mut sjis = load(&path, None).unwrap();
        sjis.lines.push("😀".into());
        assert!(save(&path, &sjis).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes.as_ref());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn save_on_windows_replaces_and_refuses_readonly() {
        let dir = tmp_dir("save-win");
        let path = dir.join("a.txt");
        fs::write(&path, "old\r\n").unwrap();

        let mut f = load(&path, None).unwrap();
        f.lines = vec!["new".into(), "行".into()];
        save(&path, &f).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\r\n行\r\n");
        // 一時ファイルが残っていない。
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        let mut perm = fs::metadata(&path).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&path, perm.clone()).unwrap();
        f.lines[0] = "changed".into();
        assert!(save(&path, &f).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\r\n行\r\n");
        perm.set_readonly(false);
        fs::set_permissions(&path, perm).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_through_symlink_keeps_the_link() {
        let dir = tmp_dir("symlink");
        let (real, link) = (dir.join("real.txt"), dir.join("link.txt"));
        fs::write(&real, "a\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut f = load(&link, None).unwrap();
        f.lines[0] = "b".into();
        save(&link, &f).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "b\n");
    }
}
