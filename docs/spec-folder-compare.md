# DiffHarbor 段階2: フォルダ比較

## 目的
2 つのフォルダを再帰的に比べ、各項目が同一か、どちらにだけあるかを一覧にする。大きなフォルダでも画面を止めない。

## スコープ
- やる: M6 比較の仕組み（diff-core）。サイズと更新日時、内容の比較、読めない項目の扱い、別スレッドでの実行、進み具合、中止、1 件の比べ直し
- やる: M7 一覧の画面（列、並べ替え、開閉、表示の切り替え、中止ボタン）
- やる: M8 タブ
- やらない: 比較方法の切り替え（内容で比べる方法のみ）、項目の削除、複数選択、除外パターンの設定

## インターフェース（M6）
```rust
pub enum Status { Identical, Different, LeftOnly, RightOnly, TypeMismatch, Error }
pub struct Meta { pub size: u64, pub modified: Option<SystemTime> }
pub struct FolderEntry {
    pub rel: PathBuf,                 // 表示用の相対パス（両側にあれば左の綴り）
    pub left_rel: Option<PathBuf>,    // 左での相対パス。左になければ None
    pub right_rel: Option<PathBuf>,
    pub name: String, pub depth: usize, pub is_dir: bool, pub status: Status,
    pub parent: Option<usize>,        // 親フォルダの項目の番号
    pub left: Option<Meta>, pub right: Option<Meta>,
    pub error: Option<String>,        // status が Error の時の理由
}
pub struct Progress; // 調べた件数と中止の指示。スレッド間で共有する
pub fn compare_dirs(left: &Path, right: &Path) -> Result<Vec<FolderEntry>>;
pub fn compare_dirs_with(left: &Path, right: &Path, progress: &Progress) -> Result<Vec<FolderEntry>>;
pub fn side_path(entries: &[FolderEntry], ix: usize, side: Side, root: &Path) -> PathBuf; // コピー先の計算にも使う
pub fn recompare(entries: &mut [FolderEntry], ix: usize, left: &Path, right: &Path);
pub fn copy_entry(src: &Path, dst: &Path) -> Result<()>;
```

## 振る舞い
| 入力 / 状態 | 期待結果 |
|---|---|
| 両側にある同じ大きさのファイル | 一定の大きさずつ読んで比べる。全体を一度に読まない |
| 両側にある大きさの違うファイル | 読まずに「差異あり」 |
| 片側がファイル、片側がフォルダ | 「種類が異なる」 |
| 読めないファイル、開けないフォルダ | その項目を「エラー」にして理由を持たせ、比較は続ける。フォルダの中身は出さない |
| 比較の起点のフォルダが開けない | 比較全体をエラーにする |
| 名前の大文字小文字だけが違う（`a.txt` と `A.txt`） | 同じ項目として比べる。表示は左の綴り |
| 同じ側に大文字小文字だけが違う名前が 2 つある | 別の項目として扱う |
| `.DS_Store`、`.git`、`Thumbs.db`、`desktop.ini`、`._*` | 比較とコピーの対象から外す |
| フォルダ（両側にある） | 中身がすべて「同一」なら「同一」、そうでなければ「差異あり」 |
| フォルダ（片側だけ） | 中身もすべて同じ側の「〜のみ」 |
| シンボリックリンク | リンク先の内容で比べる。フォルダへのリンクの中には入らない |
| 比較中 | 別スレッドで実行。調べた件数を 0.2 秒ごとに表示する。画面は止めない |
| 比較中に別の比較を始めた | 前の比較を中止し、その結果は捨てる |
| 中止 | 「中止しました」のエラーで終わる |
| 1 件の比べ直し | そのファイルの状態とサイズ、更新日時を更新し、親フォルダの状態も更新する |
| 片側だけの項目をコピー | コピー先の親フォルダの綴りに合わせた場所へコピーする |

## 未決事項
- 更新日時とサイズの表示形式（M7）。案: `2026-11-17 10:23`、バイト数を 3 桁区切り
