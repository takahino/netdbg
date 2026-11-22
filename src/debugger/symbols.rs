//! モジュールのシンボル（Portable PDB）を探す。
//!
//! 探す順:
//!   1. シンボルディレクトリの `<フォルダー>\<MVID>\symbols.pdb`。拡張が、逆コンパイルした PDB やシンボルサーバーから
//!      取ってきた PDB を置く場所で、アセンブリが持ってきたものより優先する（netcoredbg のパッチ 0009 と同じ）。
//!   2. アセンブリの隣の同名の `.pdb`。
//!   3. アセンブリに埋め込まれた PDB（`<DebugType>embedded</DebugType>`）。
//!
//! 1 と 2 は、アセンブリが覚えている PDB の ID（CodeView）と一致するものだけを使う。古い PDB を使うと、
//! 行がずれた場所で止まる。2 と 3 の順は、netcoredbg が使う System.Reflection.Metadata の
//! `TryOpenAssociatedPortablePdb` と同じ（ファイルが先、合わなければ埋め込み）。D-041。

use crate::debugger::metadata::{format_guid, Metadata};
use crate::debugger::pdb::Pdb;
use crate::debugger::target::{self, CodeView};
use std::path::{Path, PathBuf};

/// シンボルディレクトリを並べた環境変数（PATH と同じ区切り）。拡張との約束の名前（D-096 で netcoredbg の頃の名前から改めた）。
pub const SYMBOL_DIRS_VARIABLE: &str = "NETDBG_SYMBOL_DIRS";

#[derive(Default)]
pub struct Found {
    pub pdb: Option<Pdb>,
    /// PDB のファイル（埋め込み PDB なら `None`）。Edit and Continue の補助に渡す。
    pub pdb_path: Option<PathBuf>,
    /// 探す途中でアセンブリを読んだときの、そのメタデータ（あとで名前を引くのに使い回す）。
    pub metadata: Option<Metadata>,
    /// PDB があるのに使えなかった理由。利用者に伝える。
    pub problem: Option<String>,
}

/// 環境変数からシンボルディレクトリの一覧を読む。
pub fn directories_from_environment() -> Vec<PathBuf> {
    std::env::var_os(SYMBOL_DIRS_VARIABLE).map(|value| std::env::split_paths(&value).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default()
}

/// PDB が、アセンブリの覚えている ID と一致するか。どちらかに ID が無ければ、確かめようがないので通す
/// （PDB 無しでビルドしたアセンブリに、あとから作った PDB を当てる場合がこれ）。
fn matches(codeview: Option<&CodeView>, pdb: &Pdb) -> bool {
    match (codeview, pdb.id, pdb.windows_guid) {
        (Some(codeview), Some(id), _) => id[..16] == codeview.guid && id[16..] == codeview.stamp.to_le_bytes(),
        // Windows PDB は GUID だけで照らす（日時印は PDB の側に無い）。
        (Some(codeview), None, Some(guid)) => guid == codeview.guid,
        _ => true,
    }
}

pub fn find(module: &Path, symbol_directories: &[PathBuf]) -> Found {
    let adjacent = module.with_extension("pdb");
    let has_adjacent = adjacent.is_file();
    // 探す場所が無ければ、アセンブリ全体は読まない（ランタイムのモジュールは大きく、数も多い）。埋め込み PDB の有無は、
    // ヘッダーとデバッグ ディレクトリだけを読んで確かめる。
    if symbol_directories.is_empty() && !has_adjacent && !target::has_embedded_pdb(module) {
        return Found::default();
    }
    let Ok(file) = std::fs::read(module) else { return Found::default() };
    find_in(module, &file, &adjacent, symbol_directories)
}

/// ディスクに無いモジュール（単一ファイルのアプリの中のアセンブリ）の PDB。`image` は対象のメモリから読んだ像を、
/// ファイルの並びにしたもの。隣の PDB は、アプリ（exe）のフォルダーの `<名前>.pdb`（単一ファイルの発行は、そこに置く）。
pub fn find_in_memory(name: &str, image: &[u8], app_directory: Option<&Path>, symbol_directories: &[PathBuf]) -> Found {
    let adjacent = app_directory.map(|dir| dir.join(format!("{name}.pdb"))).unwrap_or_default();
    find_in(Path::new(name), image, &adjacent, symbol_directories)
}

/// アセンブリのバイト列 `file` の PDB を、シンボルディレクトリ → 隣（`adjacent`）→ 埋め込み の順に探す。
fn find_in(module: &Path, file: &[u8], adjacent: &Path, symbol_directories: &[PathBuf]) -> Found {
    let has_adjacent = adjacent.is_file();
    let codeview = target::codeview(file);
    let mut found = Found { metadata: Metadata::from_pe(file).ok(), ..Found::default() };

    if let Some(mvid) = found.metadata.as_ref().and_then(Metadata::mvid) {
        for directory in symbol_directories {
            let candidate = directory.join(format_guid(&mvid)).join("symbols.pdb");
            if !candidate.is_file() {
                continue;
            }
            match Pdb::open(&candidate) {
                Ok(pdb) if matches(codeview.as_ref(), &pdb) => {
                    found.pdb = Some(pdb);
                    found.pdb_path = Some(candidate);
                    return found;
                }
                Ok(_) => crate::log!("symbols: {} does not match {}", candidate.display(), module.display()),
                Err(e) => crate::log!("symbols: {e}"),
            }
        }
    }
    if has_adjacent {
        match Pdb::open(adjacent) {
            Ok(pdb) if matches(codeview.as_ref(), &pdb) => {
                found.pdb = Some(pdb);
                found.pdb_path = Some(adjacent.to_path_buf());
                return found;
            }
            Ok(_) => found.problem = Some(tr!("{pdb} is not from this build of {module}. Rebuild it.", pdb = adjacent.display(), module = module.display())),
            Err(e) => found.problem = Some(e),
        }
    }
    // 埋め込み PDB は同じファイルの中にあるので、ID を確かめるまでもない。使えれば、隣の PDB が使えなかったことは問題にしない。
    if let Some(entry) = target::embedded_pdb(file) {
        match Pdb::from_embedded(entry) {
            Ok(pdb) => {
                found.pdb = Some(pdb);
                found.problem = None;
            }
            Err(e) => {
                let problem = format!("{}: {e}", module.display());
                crate::log!("symbols: {problem}");
                found.problem.get_or_insert(problem);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Portable PDB（ID だけを持つ）。
    fn id(guid: u8, stamp: u32) -> Pdb {
        let mut id = [guid; 20];
        id[16..].copy_from_slice(&stamp.to_le_bytes());
        let mut pdb = Pdb::default();
        pdb.id = Some(id);
        pdb
    }

    #[test]
    fn a_pdb_of_the_same_build_matches() {
        let codeview = CodeView { guid: [7; 16], stamp: 0x1234_5678 };
        assert!(matches(Some(&codeview), &id(7, 0x1234_5678)));
    }

    #[test]
    fn a_pdb_of_another_build_does_not_match() {
        let codeview = CodeView { guid: [7; 16], stamp: 0x1234_5678 };
        assert!(!matches(Some(&codeview), &id(8, 0x1234_5678)), "GUID が違う");
        assert!(!matches(Some(&codeview), &id(7, 0x1234_5679)), "日時印が違う");
    }

    #[test]
    fn a_windows_pdb_matches_by_its_guid() {
        let codeview = CodeView { guid: [7; 16], stamp: 0x1234_5678 };
        let windows = |guid: u8| {
            let mut pdb = Pdb::default();
            pdb.windows_guid = Some([guid; 16]);
            pdb
        };
        assert!(matches(Some(&codeview), &windows(7)));
        assert!(!matches(Some(&codeview), &windows(8)));
    }

    #[test]
    fn without_an_id_on_either_side_there_is_nothing_to_compare() {
        let codeview = CodeView { guid: [7; 16], stamp: 1 };
        assert!(matches(None, &id(7, 1)));
        assert!(matches(Some(&codeview), &Pdb::default()));
    }

    #[test]
    fn a_module_that_cannot_be_read_has_no_symbols() {
        let found = find(Path::new("Z:\\no\\such\\module.dll"), &[PathBuf::from("Z:\\no\\such\\symbols")]);
        assert!(found.pdb.is_none() && found.metadata.is_none() && found.problem.is_none());
    }
}
