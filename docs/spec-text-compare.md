# DiffHarbor 段階1: テキスト比較

## 目的
2つのテキストファイルを左右に並べ、差分を見つけて移動し、ブロック単位でマージして保存できる。日本語ファイル（UTF-8 / Shift_JIS / EUC-JP）を文字化けなく比較・保存できる。

## スコープ
- やる: M1 diff-core を別クレートに分離。文字コード・改行・BOM の読み書きと、行差分、トークン差分を CLI でテストする
- やる: M2 gpui =0.2.2 の上で、明るい配色、メニュー、日本語フォントを確認する
- やる: M3 左右の整列表示、同期スクロール、単語単位の強調、パスバー、ステータスバーを作る
- やる: M4 次/前の差分移動、「差分 n / m」表示、左端の位置ペインを作る
- やる: M5 ブロック単位の左右コピー、元に戻す、保存を作る
- やらない: ペイン内の直接編集、構文ハイライト
- やらない: フォルダ比較、タブ（段階2。試作の folder.rs は保留）
- やらない: ダークテーマ・システム追従、3方向マージ、バイナリ・画像の比較

## インターフェース
```rust
pub struct TextFile { pub lines: Vec<String>, pub enc: &'static Encoding, pub eol: Eol, pub bom: bool,
    pub trailing_newline: bool, pub mixed_eol: bool, pub guessed: bool, pub lossy: bool }
pub fn load(path: &Path, force: Option<&'static Encoding>) -> Result<TextFile>;
pub fn save(path: &Path, f: &TextFile) -> Result<()>; // 変換不能文字があれば Err。一時ファイル経由
pub struct Row { pub left: Option<usize>, pub right: Option<usize>, pub kind: RowKind, pub hunk: Option<usize> }
pub struct Hunk { pub left: Range<usize>, pub right: Range<usize>, pub first_row: usize }
pub fn diff_lines(l: &[String], r: &[String]) -> DiffResult; // rows と hunks
pub fn token_spans(l: &str, r: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>); // バイト範囲
```

## 振る舞い
| 入力 / 状態 | 期待結果 |
|---|---|
| 左右が同一 | 差分 0 件。ステータスに「同一」。位置ペインは無色 |
| BOM 付き UTF-8 | BOM を保持して保存 |
| Shift_JIS / EUC-JP | 自動判定し、ステータスバーに表示。保存も同じ文字コード |
| 左右で文字コードが違う | 文字として比較。保存は各側の元の文字コード |
| UTF-8 でなく BOM もない | 推定した文字コードで開き「推定」と表示。メニューから指定して再読込 |
| UTF-16、ISO-2022-JP | 未対応としてメッセージを表示 |
| 読み込み時に変換できないバイトがある | 表示はするが保存を拒否する |
| 保存先の文字コードで表せない文字 | 保存を中止しエラー表示。ファイルは変更しない |
| NUL バイトを含む | バイナリとして開かず、メッセージを表示 |
| 改行が混在 | 比較は改行を無視。保存は最頻の改行に統一し、ステータスに「混在」と表示 |
| 片側にしかない行 | 相手側を灰色の空行で埋めて整列 |
| 行数が違う変更ブロック | 少ない側を灰色で埋める |
| 変更行 | 変わったトークンを濃い黄で強調。英数字は連続を1語、日本語は1文字単位 |
| 10万行以上 | 差分計算は別スレッド。完了まで「比較中…」を表示し、UI は止めない |
| 最後の差分で「次へ」 | 移動せず、ステータスに「最後の差分」 |
| ブロックをコピー | 対象側を更新して再比較。選択は次の差分へ。元に戻せる |
| 保存 | 同じディレクトリの一時ファイルに書き、置換する。権限は保持。失敗時は元のファイルを残す |
| 未保存のまま閉じる / 別ファイルを開く | 確認ダイアログを出す |
| 読み取り専用ファイルへ保存 | エラー表示。変更内容は画面に残す |
| シンボリックリンクへ保存 | リンクを残し、リンク先を置き換える |
| 引数にパスが2つ | 起動時に比較する |

## 未決事項
- 保存時の .bak バックアップを作るか。決める人: ユーザー。期限: M5
- 差分アルゴリズムを similar（Myers）のまま使うか、imara-diff（Histogram）にするか。実ファイルで差が出たら判断
- 操作（クリック、キー入力）の確認方法。作業環境から入力を送れない。期限: M3

## 付録: 検討経緯
- gpui は crates.io の =0.2.2 で固定と決定（2026-11-14）。試作がこの版でビルドと起動を確認済みのため
- gpui-component の最新版（0.9.1）は gpui-pre =0.3.12 に固定されており、gpui 0.2.2 と併用できない
- 段階1の UI 部品は一覧、分割ペイン、ツールバー程度で、自作できる。そのため gpui のみに依存する案を推奨する。直接編集が必要になる段階2で再判断する
- `runtime_shaders` 機能で、Metal コンパイラと Xcode のライセンス同意なしでビルドできる
- 文字コード判定には chardetng と encoding_rs を使う想定。encoding_rs は変換不能文字を黙って数値参照に置換するため、保存時に検出して中止する
