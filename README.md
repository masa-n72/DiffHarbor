# DiffHarbor

gpui で作る macOS 向けの WinMerge 風 diff/merge ツール。

## 機能

- テキストファイルの左右比較（行単位、差分ブロックの整列表示）
- 差分ブロック単位のマージ（左→右 / 右→左）、元に戻す、保存
- フォルダの再帰比較（同一 / 差異あり / 左のみ / 右のみ）、差分のみ表示
- フォルダ内の項目のコピー（左→右 / 右→左）、ダブルクリックでファイル比較

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
| ⌘S / ⌘Z | 保存 / 元に戻す |
| ⌘R | フォルダ再比較 |
| ⌘[ | フォルダ一覧へ戻る |
