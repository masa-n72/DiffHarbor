# DiffHarbor

gpui で作る macOS 向けの WinMerge 風 diff/merge ツール。

## 機能

- テキストファイルの左右比較（行単位、差分ブロックの整列表示、行内の変更箇所の強調）
- UTF-8、Shift_JIS、EUC-JP の読み書き。文字コード、改行コード、BOM を保ったまま保存
- 差分ブロック単位のマージ（左→右 / 右→左）、一括コピー、元に戻す、やり直す
- 保存、別名で保存、未保存の変更の確認
- 位置ペイン、差分の移動（次、前、先頭、末尾）
- フォルダの再帰比較（同一 / 差異あり / 左のみ / 右のみ）、差分のみ表示（試作段階）

## ビルド

Rust 1.98 以降と Xcode Command Line Tools があればビルドできます。

```bash
cargo run -- samples/ja/left_utf8.txt samples/ja/right_sjis.txt
```

テストは `cargo test --workspace` で実行します。

引数なしで起動した場合は「左を開く / 右を開く」で選択します。

開発計画は [docs/PLAN.md](docs/PLAN.md)、段階 1 の仕様は [docs/spec-text-compare.md](docs/spec-text-compare.md) にあります。

## キー操作

| キー | 動作 |
| --- | --- |
| ⌘O / ⇧⌘O | 左 / 右を開く |
| ⌥↓ / ⌥↑ | 次 / 前の差分 |
| ⌥Home / ⌥End、または ⌥⌘↑ / ⌥⌘↓ | 先頭 / 末尾の差分 |
| ⌥→ / ⌥← | 現在の差分を右 / 左へコピー |
| ⌘S | 保存 |
| ⌘Z / ⇧⌘Z | 元に戻す / やり直す |
| ⌘W | ウインドウを閉じる |
| ⌘R | フォルダ再比較 |
| ⌘[ | フォルダ一覧へ戻る |
