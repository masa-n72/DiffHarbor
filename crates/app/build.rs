//! Windows 向けに、マニフェストを実行ファイルへ埋め込む。
//!
//! gpui の `windows-manifest` 機能は gpui 自身の実行ファイルにしか効かないので、アプリ側で埋め込む。
//! マニフェストがないと、確認ダイアログ（TaskDialog）に必要な Common Controls 6 が読み込まれない。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/windows/diffharbor.rc");
        println!("cargo:rerun-if-changed=resources/windows/diffharbor.manifest.xml");
        embed_resource::compile("resources/windows/diffharbor.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
