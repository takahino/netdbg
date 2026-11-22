//! アセンブリを読めなかった理由（D-091）: `FileNotFoundException` と `FileLoadException` の記録に、要求された名前と版、
//! そのとき読み込まれている同じ名前のアセンブリの版、構成ファイル（`<exe>.config`）のバインディング リダイレクトを添える。
//! Fusion のログ（レジストリ）は有効にしない。.NET でも同じ形で添える（構成ファイルは .NET Framework のもの）。

use super::*;

/// アセンブリを読めなかった例外の型。
const LOAD_FAILURES: [&str; 2] = ["System.IO.FileNotFoundException", "System.IO.FileLoadException"];

/// 要求された名前（`Name, Version=1.2.0.0, Culture=neutral, ...` の名前と版。ファイルのパスなら、ファイル名から拡張子を
/// 除いたもの）。
pub(super) fn requested(text: &str) -> (String, Option<String>) {
    let mut parts = text.split(',').map(str::trim);
    let first = parts.next().unwrap_or("");
    let name = if first.contains(['\\', '/']) || first.to_ascii_lowercase().ends_with(".dll") || first.to_ascii_lowercase().ends_with(".exe") {
        let file = first.rsplit(['\\', '/']).next().unwrap_or(first);
        file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string()
    } else {
        first.to_string()
    };
    let version = parts.find_map(|p| p.strip_prefix("Version=").map(str::to_string));
    (name, version)
}

/// 構成ファイルのバインディング リダイレクト 1 つ。
#[derive(Debug, PartialEq)]
pub(super) struct Redirect {
    pub name: String,
    pub old_version: String,
    pub new_version: String,
}

/// 属性の値（`name="..."`）。
fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let at = element.find(&format!("{name}=\""))? + name.len() + 2;
    let rest = &element[at..];
    Some(&rest[..rest.find('"')?])
}

/// 構成ファイルの `<dependentAssembly>` ごとの `assemblyIdentity` の名前と `bindingRedirect` の版。
pub(super) fn binding_redirects(config: &str) -> Vec<Redirect> {
    let mut redirects = Vec::new();
    for block in config.split("<dependentAssembly").skip(1) {
        let block = block.split("</dependentAssembly>").next().unwrap_or(block);
        let identity = block.split("<assemblyIdentity").nth(1).and_then(|e| e.split('>').next());
        let redirect = block.split("<bindingRedirect").nth(1).and_then(|e| e.split('>').next());
        if let (Some(identity), Some(redirect)) = (identity, redirect) {
            if let (Some(name), Some(old_version), Some(new_version)) = (attribute(identity, "name"), attribute(redirect, "oldVersion"), attribute(redirect, "newVersion")) {
                redirects.push(Redirect { name: name.to_string(), old_version: old_version.to_string(), new_version: new_version.to_string() });
            }
        }
    }
    redirects
}

impl Session {
    /// アセンブリを読めなかった例外なら、その理由の材料（記録の `assemblyLoad`）と、デバッグ コンソールに足す 1 行。
    pub(super) fn assembly_load_details(&self, type_name: &str, exception: &ComPtr<ICorDebugValue>) -> Option<(Value, String)> {
        if !LOAD_FAILURES.contains(&type_name) {
            return None;
        }
        let text = unsafe { value::string_field(exception, &self.modules, &["_fileName", "<FileName>k__BackingField", "m_fileName"]) }?;
        let (name, version) = requested(&text);
        let loaded: Vec<Value> = self
            .modules
            .iter()
            .filter_map(|m| m.metadata().and_then(|md| md.assembly_identity()).map(|(n, v)| (m, n, v)))
            .filter(|(_, n, _)| n.eq_ignore_ascii_case(&name))
            .map(|(m, _, v)| json!({ "path": m.path, "version": v }))
            .collect();
        let config = self.program_path().map(|p| format!("{p}.config"));
        let redirects: Vec<Value> = config
            .as_deref()
            .and_then(|c| std::fs::read_to_string(c).ok())
            .map(|xml| binding_redirects(&xml).into_iter().filter(|r| r.name.eq_ignore_ascii_case(&name)).map(|r| json!({ "oldVersion": r.old_version, "newVersion": r.new_version })).collect())
            .unwrap_or_default();
        let loaded_text = if loaded.is_empty() { tr!("none") } else { loaded.iter().filter_map(|l| l["version"].as_str()).collect::<Vec<_>>().join(", ") };
        let redirect_text = if redirects.is_empty() { tr!("none") } else { redirects.iter().map(|r| format!("{} → {}", r["oldVersion"].as_str().unwrap_or(""), r["newVersion"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ") };
        let line = tr!("  Requested: {name} {version}; already loaded: {loaded}; binding redirect: {redirect}", name = name, version = version.clone().unwrap_or_default(), loaded = loaded_text, redirect = redirect_text);
        let body = json!({ "requested": { "text": text, "name": name, "version": version }, "loaded": loaded, "redirects": redirects, "configFile": config });
        Some((body, line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_requested_name_and_version_are_read_from_the_display_name_or_a_path() {
        assert_eq!(requested("Contoso.Data, Version=2.1.0.0, Culture=neutral, PublicKeyToken=null"), ("Contoso.Data".into(), Some("2.1.0.0".into())));
        assert_eq!(requested(r"C:\app\Contoso.Data.dll"), ("Contoso.Data".into(), None));
        assert_eq!(requested("Contoso.Data"), ("Contoso.Data".into(), None));
    }

    #[test]
    fn binding_redirects_are_read_per_dependent_assembly() {
        let config = r#"<configuration><runtime><assemblyBinding xmlns="urn:schemas-microsoft-com:asm.v1">
            <dependentAssembly><assemblyIdentity name="Contoso.Data" publicKeyToken="abc" culture="neutral" />
                <bindingRedirect oldVersion="0.0.0.0-2.0.0.0" newVersion="2.0.0.0" /></dependentAssembly>
            <dependentAssembly><assemblyIdentity name="Other" /></dependentAssembly>
        </assemblyBinding></runtime></configuration>"#;
        assert_eq!(binding_redirects(config), vec![Redirect { name: "Contoso.Data".into(), old_version: "0.0.0.0-2.0.0.0".into(), new_version: "2.0.0.0".into() }]);
    }
}
