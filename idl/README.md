# idl

`cordebug.idl` は dotnet/runtime の `src/coreclr/inc/cordebug.idl` をそのまま置いたもの(MIT License、.NET Foundation)。

- 取得元: https://github.com/dotnet/runtime
- コミット: `56d5bb652aced6bb4fed74ab43fbdd4c39b6b7ed`(release/10.0)
- SHA-256: `AB799E1E35DE361AD479D0552D5E737177F8EC21170B9F487913B16B523A69AC`

`src/cordebug.rs` はここから `tools/idlgen` が生成する。更新するときは、ファイルを差し替えてこの 2 行を書き換え、次を実行する。

```powershell
cargo run -p idlgen -- idl/cordebug.idl src/cordebug.rs
cargo test    # src/cordebug_layout_test.rs が vtable の並びを検算する
```

## metahost.idl

`metahost.idl` は同じコミットの `src/coreclr/inc/metahost.idl`(.NET Framework の ICorDebug を受け取る `ICLRMetaHost` / `ICLRRuntimeInfo`)。

- SHA-256: `DA227AC8784E727DA517644925DF7C4B97F725424B7C345B260F140ED303A47B`

```powershell
cargo run -p idlgen -- idl/metahost.idl src/metahost.rs
```

## oleacc.idl

`oleacc.idl` は、Active Accessibility の `IAccessible`(と基底の `IDispatch`)のうち、ここで使う宣言を、文書(learn.microsoft.com の IAccessible)のメソッドの並びのとおりに書いたもの。対象の画面のボタンの仲間の役割と状態を読むのに使う(D-054)。`VARIANT` は、値の部分を `ULONG_PTR` 2 つにして、Windows と同じ大きさ(x86 で 16、x64 で 24 バイト)にしてある。

```powershell
cargo run -p idlgen -- idl/oleacc.idl src/oleacc.rs
cargo test    # src/cordebug_layout_test.rs が vtable の並びを検算する
```

## uiautomation.idl

`uiautomation.idl` は、Windows SDK 10.0.26100.0 の `um\UIAutomationClient.idl` から、WPF の画面を読み、操作するのに使うインターフェイス(`IUIAutomation` / `IUIAutomation2`、`IUIAutomationElement`、要素の配列、条件、CacheRequest、Invoke / Value / Toggle / Selection / SelectionItem / ExpandCollapse の各パターン)と、番号の `module`(パターン、プロパティ、コントロールの種類)を、SDK の文面のまま写したもの(D-055)。列挙の `ToggleState` と `ExpandCollapseState` は同じ SDK の `UIAutomationCore.idl` から。SDK では `library` の中にあるものを、idlgen が読める最上位に置き、型の別名(`BSTR`、`PROPERTYID` など)と `RECT` / `POINT` / `VARIANT` を先頭に足してある。

- 写したもとの SHA-256(`UIAutomationClient.idl`): `B79EFF8E7357F51BEA749611B365A61F969ED70F3DBCB9F2C4FAD97678286F1F`
- `src/cordebug_layout_test.rs` は、SDK の C のヘッダー(`UIAutomationClient.h`)の Vtbl の並びから数えた番号で検算する(生成元の idl とは別の資料で確かめるため)。

```powershell
Get-FileHash "C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um\UIAutomationClient.idl"
cargo run -p idlgen -- idl/uiautomation.idl src/uiautomation.rs
cargo test
```