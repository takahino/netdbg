//! 専用の画面（段階 13）のファイルを、エンジンの exe に埋め込む表を作る（`src/interface/ui/assets.rs` が読む）。
//!
//! `ui/build.ps1` の出力（`target/ui/dist`）があれば、そのファイルを `include_bytes!` で埋め込む。無ければ、ビルドの
//! しかたを書いたページだけにする（wasm の道具の無い機械でも、エンジンはビルドでき、試験も通る）。
//!
//! あわせて、exe のアイコン（`assets/netdbg.ico`）を、リソース（.res）にして MSVC のリンカーに渡す。.res は
//! ここで書く（rc.exe も、リソースのクレートも要らない。リンカーが .res を受け取って埋め込む）。

use std::fmt::Write as _;
use std::path::Path;

/// 埋め込むファイルと、その種類。
const FILES: [(&str, &str); 5] = [
    ("index.html", "text/html; charset=utf-8"),
    ("icon.svg", "image/svg+xml"),
    ("style.css", "text/css; charset=utf-8"),
    ("ui.js", "text/javascript; charset=utf-8"),
    ("ui_bg.wasm", "application/wasm"),
];

const PLACEHOLDER: &str = "<!DOCTYPE html><html lang=\"ja\"><head><meta charset=\"utf-8\"><title>netdbg</title></head><body>\
<p>画面がビルドされていません。エンジンのリポジトリで <code>./ui/build.ps1</code> を実行してから、エンジンをビルドし直してください。</p>\
<p>The UI is not built. Run <code>./ui/build.ps1</code> in the engine repository, then rebuild the engine.</p></body></html>";

fn main() {
    embed_icon();
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("ui").join("dist");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut table = String::from("pub static ASSETS: &[(&str, &str, &[u8])] = &[\n");
    let built = FILES.iter().all(|(name, _)| dist.join(name).is_file());
    if built {
        for (name, kind) in FILES {
            let file = dist.join(name);
            println!("cargo:rerun-if-changed={}", file.display());
            writeln!(table, "    (\"/{name}\", \"{kind}\", include_bytes!({:?})),", file.display().to_string()).unwrap();
        }
    } else {
        writeln!(table, "    (\"/index.html\", \"text/html; charset=utf-8\", {:?}.as_bytes()),", PLACEHOLDER).unwrap();
    }
    table.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("ui_assets.rs");
    std::fs::write(out, table).unwrap();
}

/// `assets/netdbg.ico` を、exe のアイコンのリソースにして、リンカーに渡す（MSVC だけ）。
fn embed_icon() {
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("netdbg.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let bytes = std::fs::read(&ico).unwrap_or_else(|e| panic!("{}: {e}", ico.display()));
    let res = Path::new(&std::env::var("OUT_DIR").unwrap()).join("netdbg.res");
    std::fs::write(&res, icon_resources(&bytes)).unwrap();
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// .ico の中身を、.res（RT_ICON を絵ごとに 1 つと、それをまとめる RT_GROUP_ICON の 1 番）にする。
fn icon_resources(ico: &[u8]) -> Vec<u8> {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    let word = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let dword = |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]);
    let count = word(4) as usize;
    let mut res = Vec::new();
    // .res の始まりの、中身の無い 1 つ（32 バイトの頭だけ）。
    resource(&mut res, 0, 0, 0, &[]);
    let mut group = vec![0, 0, 1, 0];
    group.extend_from_slice(&(count as u16).to_le_bytes());
    for index in 0..count {
        let entry = 6 + index * 16;
        let (size, offset) = (dword(entry + 8) as usize, dword(entry + 12) as usize);
        let id = index as u16 + 1;
        resource(&mut res, RT_ICON, id, 0x1010, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: .ico の ICONDIRENTRY の始めの 12 バイトと、リソースの番号。
        group.extend_from_slice(&ico[entry..entry + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut res, RT_GROUP_ICON, 1, 0x1030, &group);
    res
}

/// .res に 1 つ書く（頭は 32 バイト。種類と名前は番号。中身は 4 バイト境界まで埋める）。
fn resource(res: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    res.extend_from_slice(&(data.len() as u32).to_le_bytes());
    res.extend_from_slice(&32u32.to_le_bytes());
    for value in [0xFFFF, kind, 0xFFFF, id] {
        res.extend_from_slice(&value.to_le_bytes());
    }
    res.extend_from_slice(&0u32.to_le_bytes());
    res.extend_from_slice(&flags.to_le_bytes());
    res.extend_from_slice(&0u16.to_le_bytes());
    res.extend_from_slice(&[0; 8]);
    res.extend_from_slice(data);
    res.resize(res.len().div_ceil(4) * 4, 0);
}