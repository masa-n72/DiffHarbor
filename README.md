# DiffHarbor

gpui で作る macOS と Windows 向けの WinMerge 風 diff/merge ツール。

## 機能

- テキストファイルの左右比較（行単位、差分ブロックの整列表示、行内の変更箇所の強調）
- UTF-8、Shift_JIS、EUC-JP の読み書き。文字コード、改行コード、BOM を保ったまま保存
- 差分ブロック単位のマージ（左→右 / 右→左）、一括コピー、元に戻す、やり直す
- 保存、別名で保存、未保存の変更の確認
- 位置ペイン、差分の移動（次、前、先頭、末尾）
- INI ファイルのキー単位の比較（並び順、空白、コメント、大文字小文字の違いを無視）。キーごとのコピーと保存
- フォルダの再帰比較（同一 / 差異あり / 左のみ / 右のみ）。更新日時とサイズの列、並べ替え、フォルダの開閉、表示の切り替え

## ビルド

Rust 1.98 以降が必要です。

- macOS: Xcode Command Line Tools
- Windows: MSVC 版の Rust（`x86_64-pc-windows-msvc`）と、Visual Studio Build Tools の「C++ によるデスクトップ開発」（MSVC と Windows SDK）。リリースビルドでは Windows SDK の `fxc.exe` を使います。見つからない場合は環境変数 `GPUI_FXC_PATH` に場所を指定します

```bash
cargo run -- samples/ja/left_utf8.txt samples/ja/right_sjis.txt
```

テストは `cargo test --workspace` で実行します。

引数なしで起動した場合は「左を開く / 右を開く」で選択します。

開発計画は [docs/PLAN.md](docs/PLAN.md)、段階 1 の仕様は [docs/spec-text-compare.md](docs/spec-text-compare.md) にあります。

## キー操作

| macOS | Windows | 動作 |
| --- | --- | --- |
| ⌘O / ⇧⌘O | Ctrl+O / Ctrl+Shift+O | 左 / 右を開く |
| ⌥↓ / ⌥↑ | Alt+↓ / Alt+↑ | 次 / 前の差分 |
| ⌥Home / ⌥End、または ⌥⌘↑ / ⌥⌘↓ | Alt+Home / Alt+End、または Ctrl+Alt+↑ / Ctrl+Alt+↓ | 先頭 / 末尾の差分 |
| ⌥→ / ⌥← | Alt+→ / Alt+← | 現在の差分を右 / 左へコピー |
| ⌘S | Ctrl+S | 保存 |
| ⌘Z / ⇧⌘Z | Ctrl+Z / Ctrl+Shift+Z、Ctrl+Y | 元に戻す / やり直す |
| ⌘W | Ctrl+W | ウインドウを閉じる |
| ⌘R | Ctrl+R | フォルダ再比較 |
| ⌘[ | Ctrl+[ | フォルダ一覧へ戻る |
| ↑ / ↓、← / →、Enter | 同じ | フォルダ一覧: 選択の移動、フォルダの閉じる / 開く、開く |
| Esc | Esc | フォルダ比較の中止 |

Windows ではメニューをウィンドウ上部に表示します。
