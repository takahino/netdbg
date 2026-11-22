<#
.SYNOPSIS
    netdbg を x86 と x64 で release ビルドし、dist\ に同梱の形で並べる。
.DESCRIPTION
    必要なもの: Rust (rustup) と MSVC のリンカー。ターゲット i686-pc-windows-msvc と x86_64-pc-windows-msvc。
    専用の画面のために、ターゲット wasm32-unknown-unknown と wasm-bindgen-cli (ui\build.ps1)。
    出力:
        dist\x86\netdbg.exe   主
        dist\x64\netdbg.exe
.PARAMETER Arch
    ビルドするビット数。既定は両方。
#>
[CmdletBinding()]
param(
    [ValidateSet('x86', 'x64')]
    [string[]]$Arch = @('x86', 'x64')
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$targets = @{ x86 = 'i686-pc-windows-msvc'; x64 = 'x86_64-pc-windows-msvc' }

# dbgshim.dll (.NET のプロセスを起動して ICorDebug を受け取るための Microsoft の DLL、MIT) は NuGet から取得する。
# 版と中身を固定する。更新するときは版を変え、取得した DLL のハッシュをここに書き、スモーク試験を通す。
$dbgshimVersion = '10.0.750501'
$dbgshimSha256 = @{
    x86 = 'B79449715FA62ABA401072FF0AEB5577B09CC25AD8F14670EB84263C55E8960C'
    x64 = '18730F1FE4B555AB8F062E4C14104EE64CAC774C9277554E9CC560546ABD22DF'
}

function Get-Dbgshim([string]$a) {
    $cache = Join-Path $root "target\dbgshim\$dbgshimVersion\$a"
    $dll = Join-Path $cache 'dbgshim.dll'
    if (-not (Test-Path $dll)) {
        $package = "microsoft.diagnostics.dbgshim.win-$a"
        $url = "https://api.nuget.org/v3-flatcontainer/$package/$dbgshimVersion/$package.$dbgshimVersion.nupkg"
        New-Item -ItemType Directory -Force $cache | Out-Null
        $zip = Join-Path $cache 'package.zip'
        Write-Host "==> $url" -ForegroundColor Cyan
        try {
            Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        } catch {
            throw "dbgshim の取得に失敗しました: $url : $($_.Exception.Message)。ネットワークを確認するか、同じ版の dbgshim.dll を $dll に置いてください。"
        }
        $extracted = Join-Path $cache 'package'
        Expand-Archive -Path $zip -DestinationPath $extracted -Force
        Copy-Item (Join-Path $extracted "runtimes\win-$a\native\dbgshim.dll") $dll
        Remove-Item -Recurse -Force $zip, $extracted
    }
    $actual = (Get-FileHash $dll -Algorithm SHA256).Hash
    if ($actual -ne $dbgshimSha256[$a]) {
        throw "dbgshim.dll ($a) のハッシュが違います: $actual (期待: $($dbgshimSha256[$a]))。$dll を消して取得し直してください。"
    }
    return $dll
}

# Edit and Continue の差分を作る補助 (tools/encdelta。D-059)。ビット数を問わないので一度だけビルドし、各 dist に写す。
# Roslyn は同梱しない (動くときに、利用者の SDK のものを読み込む)。
Write-Host '==> dotnet build tools/encdelta' -ForegroundColor Cyan
$encdelta = Join-Path $root 'target\encdelta'
if (Test-Path $encdelta) { Remove-Item -Recurse -Force $encdelta }
$ErrorActionPreference = 'Continue'
& dotnet build (Join-Path $root 'tools\encdelta') -c Release -o $encdelta -nologo -v q
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) {
    throw "tools/encdelta のビルドに失敗しました (exit code $LASTEXITCODE)。.NET SDK 9 以降と、nuget.org への接続 (Roslyn の参照のため) が要ります。"
}

# 専用の画面 (段階 13、D-062)。エンジンのビルド (build.rs) が、その出力を exe に埋め込むので、先に作る。
& (Join-Path $root 'ui\build.ps1')

$installed = & rustup target list --installed
foreach ($a in $Arch) {
    $target = $targets[$a]
    if ($installed -notcontains $target) {
        throw "Rust のターゲット $target がありません。'rustup target add $target' を実行してください。"
    }
    Write-Host "==> cargo build --release --target $target" -ForegroundColor Cyan
    # cargo は進捗を標準エラーに出す。Windows PowerShell 5.1 は、標準エラーが転送されていると 'Stop' の下で
    # それを例外にしてしまうので、この呼び出しの間だけ 'Continue' にして終了コードで成否を判定する。
    $ErrorActionPreference = 'Continue'
    & cargo build --release --target $target --manifest-path (Join-Path $root 'Cargo.toml')
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        throw "$target のビルドに失敗しました (exit code $LASTEXITCODE)。上のログを確認してください。"
    }
    $dest = Join-Path $root "dist\$a"
    New-Item -ItemType Directory -Force $dest | Out-Null
    Copy-Item (Join-Path $root "target\$target\release\netdbg.exe") $dest -Force
    Copy-Item (Get-Dbgshim $a) $dest -Force
    $destEncdelta = Join-Path $dest 'encdelta'
    if (Test-Path $destEncdelta) { Remove-Item -Recurse -Force $destEncdelta }
    New-Item -ItemType Directory -Force $destEncdelta | Out-Null
    Get-ChildItem $encdelta -File | Where-Object { $_.Extension -in '.dll', '.json' } | Copy-Item -Destination $destEncdelta -Force
    $exe = Get-Item (Join-Path $dest 'netdbg.exe')
    Write-Host ("    {0}  {1:N0} bytes" -f $exe.FullName, $exe.Length)
}
