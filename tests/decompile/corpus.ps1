<#
.SYNOPSIS
    コーパスの 4 グループをビルドし、逆コンパイルで戻せた割合と ILSpy との差を出す（D-105）。
.DESCRIPTION
    tests/corpus/repositories.json を取得し、dotnet build（既定の Debug）する。
    ビルドできたプロジェクトの出力だけを、エンジンの --decompile と ILSpy で戻して比べる。
    ビルドできないものは target/decompile-corpus/skipped.tsv に理由を残し、割合に入れない。
    結果は target/decompile-corpus/ に書く（コミットしない）。
.PARAMETER Exe
    使うエンジン。既定は、release をビルドした target\i686-pc-windows-msvc\release\netdbg.exe。
.PARAMETER Group
    このグループだけ（famous / old / middle / minor）。省略すると 4 つとも。
.PARAMETER Refresh
    取得し直す。既定では、取得済みのソースはそのまま使う（ビルドは毎回する）。
#>
[CmdletBinding()]
param(
    [string]$Exe = '',
    [string]$Group = '',
    [switch]$Refresh
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$utf8 = New-Object System.Text.UTF8Encoding $false
$utf8Bom = New-Object System.Text.UTF8Encoding $true
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Write-Utf8([string]$Path, [string]$Text, [switch]$Bom) {
    $encoding = if ($Bom) { $utf8Bom } else { $utf8 }
    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    [IO.File]::WriteAllText($Path, $Text, $encoding)
}

function Add-Utf8Line([string]$Path, [string]$Line) {
    [IO.File]::AppendAllText($Path, $Line + "`n", $utf8)
}

function Invoke-Native {
    param([scriptblock]$Command, [string]$Log = '')
    # 関数の戻りに、コマンドの表示を混ぜない（混ぜると終了コードの判定が壊れる）。
    # 標準エラーは文字列にしてから出す（PowerShell 5.1 がエラーレコードに包む）。
    $ErrorActionPreference = 'Continue'
    $lines = @(& $Command 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($Log -and $lines.Count -gt 0) {
        [IO.File]::AppendAllText($Log, (($lines -join "`n") + "`n"), $utf8)
    } elseif ($lines.Count -gt 0) {
        $lines | Out-Host
    }
    if ($null -eq $code) { $code = 0 }
    return [int]$code
}

if (-not $Exe) {
    Write-Host '==> cargo build --release（逆コンパイルは、いまのソースの release）' -ForegroundColor Cyan
    Push-Location $root
    try {
        $code = Invoke-Native { cargo build --release --manifest-path (Join-Path $root 'Cargo.toml') }
        if ($code -ne 0) { throw "cargo build --release が失敗しました (exit code $code)。" }
    } finally { Pop-Location }
    $Exe = Join-Path $root 'target\i686-pc-windows-msvc\release\netdbg.exe'
    if (-not (Test-Path -LiteralPath $Exe)) { $Exe = Join-Path $root 'target\release\netdbg.exe' }
}
if (-not (Test-Path -LiteralPath $Exe)) { throw "エンジンがありません: $Exe" }

$measureProj = Join-Path $root 'tools\decompile-measure\decompile-measure.csproj'
$measureDir = Join-Path $root 'target\decompile-measure'
Write-Host '==> dotnet build tools/decompile-measure' -ForegroundColor Cyan
$code = Invoke-Native { dotnet build $measureProj -c Release -o $measureDir -nologo -v q }
if ($code -ne 0) { throw "tools/decompile-measure のビルドが失敗しました (exit code $code)。" }
$measure = Join-Path $measureDir 'decompile-measure.exe'
if (-not (Test-Path -LiteralPath $measure)) { throw "decompile-measure.exe がありません。" }

$work = Join-Path $root 'target\decompile-corpus'
$srcRoot = Join-Path $work 'src'
$logRoot = Join-Path $work 'logs'
New-Item -ItemType Directory -Force -Path $srcRoot, $logRoot | Out-Null
$skippedPath = Join-Path $work 'skipped.tsv'
$builtPath = Join-Path $work 'built.tsv'
Write-Utf8 $skippedPath "group`trepo`treason`n"
Write-Utf8 $builtPath "group`trepo`tassembly`tpath`n"

$env:NETDBG_LANG = 'en'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_NOLOGO = '1'
$env:NUGET_CERT_REVOCATION_MODE = 'offline'

function Get-NamedDirs([string]$Root, [string[]]$Names) {
    $found = New-Object System.Collections.Generic.List[string]
    if (-not (Test-Path -LiteralPath $Root)) { return @() }
    $stack = New-Object System.Collections.Generic.Stack[string]
    $stack.Push($Root)
    while ($stack.Count -gt 0) {
        $dir = $stack.Pop()
        try {
            foreach ($sub in [IO.Directory]::EnumerateDirectories($dir)) {
                $name = [IO.Path]::GetFileName($sub)
                if ($Names -contains $name) { $found.Add($sub) }
                elseif ($name -ne 'packages' -and $name -ne 'node_modules' -and $name -ne '.git') { $stack.Push($sub) }
            }
        } catch { }
    }
    return @($found)
}

function Remove-BuildDirs([string]$Root) {
    if (-not (Test-Path -LiteralPath $Root)) { return }
    $stack = New-Object System.Collections.Generic.Stack[string]
    $stack.Push($Root)
    while ($stack.Count -gt 0) {
        $dir = $stack.Pop()
        try {
            foreach ($sub in [IO.Directory]::EnumerateDirectories($dir)) {
                $name = [IO.Path]::GetFileName($sub)
                if ($name -eq 'bin' -or $name -eq 'obj') {
                    try { Remove-Item -LiteralPath $sub -Recurse -Force -ErrorAction SilentlyContinue } catch { }
                } elseif ($name -ne 'packages' -and $name -ne 'node_modules' -and $name -ne '.git') {
                    $stack.Push($sub)
                }
            }
        } catch { }
    }
}

function Get-SafeChildren([string]$Root, [string]$Filter) {
    # 生成ファイルのパスが MAX_PATH を超えても、そこ以外のファイルは残す。
    $found = New-Object System.Collections.Generic.List[string]
    if (-not (Test-Path -LiteralPath $Root)) { return @() }
    $stack = New-Object System.Collections.Generic.Stack[string]
    $stack.Push($Root)
    while ($stack.Count -gt 0) {
        $dir = $stack.Pop()
        try {
            foreach ($sub in [IO.Directory]::EnumerateDirectories($dir)) {
                $name = [IO.Path]::GetFileName($sub)
                if ($name -eq 'packages' -or $name -eq 'node_modules' -or $name -eq '.git') { continue }
                $stack.Push($sub)
            }
            foreach ($file in [IO.Directory]::EnumerateFiles($dir, $Filter)) { $found.Add($file) }
        } catch { }
    }
    return @($found)
}

function Get-AssemblyName([string]$Project) {
    $text = [IO.File]::ReadAllText($Project)
    if ($text -match '<AssemblyName>\s*([^<]+)\s*</AssemblyName>') {
        $name = $Matches[1].Trim()
        if ($name -notmatch '\$\(') { return $name }
    }
    return [IO.Path]::GetFileNameWithoutExtension($Project)
}

function Test-PlatformName([string]$Name) {
    $n = $Name.ToLowerInvariant()
    foreach ($prefix in @('system.', 'microsoft.', 'mscorlib', 'netstandard', 'windowsbase', 'presentationcore', 'presentationframework', 'uiautomation', 'accessibility')) {
        if ($n.StartsWith($prefix)) { return $true }
    }
    return $false
}

function Get-TfmRank([string]$Path) {
    $tfm = $null
    foreach ($part in $Path.Split('\')) {
        if ($part -match '^(net\d|netstandard|netcoreapp)') { $tfm = $part }
    }
    if (-not $tfm) { return 1000 }
    $rank = 1000
    if ($tfm -match '^net(\d+)\.(\d+)') { $rank = 5000 + [int]$Matches[1] * 100 + [int]$Matches[2] }
    elseif ($tfm -match '^netcoreapp(\d+)\.(\d+)') { $rank = 4000 + [int]$Matches[1] * 100 + [int]$Matches[2] }
    elseif ($tfm -match '^net(\d+)') { $rank = 3000 + [int]$Matches[1] }
    elseif ($tfm -match '^netstandard(\d+)\.(\d+)') { $rank = 2000 + [int]$Matches[1] * 100 + [int]$Matches[2] }
    if ($tfm -match '-') { $rank -= 1 }
    return $rank
}

function Get-GroupKey([string]$Path) {
    $key = [regex]::Replace($Path, '\\(net\d[\w.\-]*|netstandard[\w.\-]*|netcoreapp[\w.\-]*)\\', '\')
    return $key.ToLowerInvariant()
}

function Get-FirstError([string]$Log) {
    if (-not (Test-Path -LiteralPath $Log)) { return 'ビルドの出力が無い' }
    $line = Select-String -LiteralPath $Log -Pattern ': error |error MSB|error NU' | Select-Object -First 1
    if ($line) {
        $text = ($line.Line -replace '\s+', ' ').Trim()
        if ($text.Length -gt 180) { $text = $text.Substring(0, 180) }
        return $text
    }
    $tail = Get-Content -LiteralPath $Log -Tail 1 -ErrorAction SilentlyContinue
    if ($tail) { return (($tail -replace '\s+', ' ').Trim()) }
    return 'ビルドに失敗し、エラー行が無い'
}

function Get-BuiltAssemblies([string]$RepoDir) {
    # そのプロジェクトの bin にある、そのプロジェクトの名前の出力だけ。他のプロジェクトへコピーされたものは数えない。
    $projects = @(Get-SafeChildren $RepoDir '*.csproj' | Where-Object { $_ -notmatch '\\(packages|node_modules|\.git)\\' })
    $best = @{}
    $seen = @{}
    foreach ($projPath in $projects) {
        $proj = Get-Item -LiteralPath $projPath
        $name = Get-AssemblyName $proj.FullName
        if (Test-PlatformName $name) { continue }
        $bin = Join-Path $proj.DirectoryName 'bin'
        if (-not (Test-Path -LiteralPath $bin)) { continue }
        $found = @(Get-ChildItem -LiteralPath $bin -Recurse -File -ErrorAction SilentlyContinue | Where-Object {
            ($_.Extension -eq '.dll' -or $_.Extension -eq '.exe') -and $_.FullName -notmatch '\\ref\\' -and ($_.BaseName -eq $name)
        })
        foreach ($file in $found) {
            $key = (Get-GroupKey $file.FullName)
            $rank = Get-TfmRank $file.FullName
            if (-not $best.ContainsKey($key) -or $rank -gt $best[$key].Rank) {
                $best[$key] = @{ Path = $file.FullName; Rank = $rank; Name = $file.Name }
            }
            $seen[$name.ToLowerInvariant()] = $true
        }
    }
    # 出力先を bin の外にしている csproj は、名前が一致するファイルを 1 つだけ拾う。
    $bins = @(Get-NamedDirs $RepoDir @('bin') | Where-Object { $_ -notmatch '\\(packages|node_modules|\.git)\\' })
    foreach ($projPath in $projects) {
        $name = Get-AssemblyName $projPath
        if (Test-PlatformName $name) { continue }
        if ($seen.ContainsKey($name.ToLowerInvariant())) { continue }
        foreach ($bin in $bins) {
            $found = @(Get-ChildItem -LiteralPath $bin -Recurse -File -ErrorAction SilentlyContinue | Where-Object {
                ($_.Extension -eq '.dll' -or $_.Extension -eq '.exe') -and $_.FullName -notmatch '\\ref\\' -and ($_.BaseName -eq $name)
            })
            foreach ($file in $found) {
                $key = $name.ToLowerInvariant()
                $rank = Get-TfmRank $file.FullName
                if (-not $best.ContainsKey($key) -or $rank -gt $best[$key].Rank) {
                    $best[$key] = @{ Path = $file.FullName; Rank = $rank; Name = $file.Name }
                }
            }
        }
    }
    return @($best.Values)
}

function Invoke-RepoBuild([string]$RepoDir, [string]$Log) {
    if (Test-Path -LiteralPath $Log) { Remove-Item -LiteralPath $Log -Force }
    Remove-BuildDirs $RepoDir
    $slns = @(Get-SafeChildren $RepoDir '*.sln' | Where-Object { $_ -notmatch '\\(packages|node_modules|\.git)\\' })
    $projects = @(Get-SafeChildren $RepoDir '*.csproj' | Where-Object { $_ -notmatch '\\(packages|node_modules|\.git)\\' })
    $targets = @()
    if ($slns.Count -gt 0) { $targets = $slns } else { $targets = $projects }
    foreach ($target in $targets) {
        $code = Invoke-Native -Log $Log -Command { dotnet build $target -c Debug -nologo -v q }
        "exit $code  $target" | Out-File -FilePath $Log -Append -Encoding utf8
    }
    $outputs = @(Get-BuiltAssemblies $RepoDir)
    if ($outputs.Count -eq 0 -and $slns.Count -gt 0) {
        foreach ($proj in $projects) {
            $code = Invoke-Native -Log $Log -Command { dotnet build $proj -c Debug -nologo -v q }
            "exit $code  $proj" | Out-File -FilePath $Log -Append -Encoding utf8
        }
        $outputs = @(Get-BuiltAssemblies $RepoDir)
    }
    return $outputs
}

function Expand-Repo([string]$Zip, [string]$Dest) {
    if (Test-Path -LiteralPath $Dest) { Remove-Item -LiteralPath $Dest -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $rootName = $null
        foreach ($entry in $archive.Entries) {
            if (-not $rootName) { $rootName = $entry.FullName.Split('/')[0] }
            if ($entry.FullName.EndsWith('/')) { continue }
            if (-not $entry.FullName.StartsWith($rootName + '/')) { continue }
            $rel = $entry.FullName.Substring($rootName.Length + 1)
            if ($rel.Contains('..')) { continue }
            $target = Join-Path $Dest ($rel -replace '/', '\')
            $parent = Split-Path -Parent $target
            if ($parent -and -not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    } finally { $archive.Dispose() }
}

$groups = Get-Content (Join-Path $root 'tests\corpus\repositories.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$names = @($groups.PSObject.Properties.Name)
if ($Group) { $names = @($names | Where-Object { $_ -eq $Group }) }
if ($names.Count -eq 0) { throw "グループがありません: $Group" }

$speed = @{}
$invariant = [Globalization.CultureInfo]::InvariantCulture
function Add-Times([string]$GroupName, [string[]]$Lines) {
    foreach ($line in $Lines) {
        if ($line -match 'decompiled in ([0-9.]+) ms') { $speed[$GroupName].Engine += [double]::Parse($Matches[1], $invariant) }
        if ($line -match 'ilspy-ms ([0-9.]+)') { $speed[$GroupName].Ilspy += [double]::Parse($Matches[1], $invariant) }
    }
}
function Invoke-Captured([scriptblock]$Command) {
    $ErrorActionPreference = 'Continue'
    $lines = @(& $Command 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 0 }
    return @{ Code = [int]$code; Lines = $lines }
}

foreach ($name in $names) {
    $speed[$name] = @{ Engine = 0.0; Ilspy = 0.0 }
    $tsv = Join-Path $work "$name.tsv"
    Write-Utf8 $tsv "group`trepo`tassembly`ttype`tmethod`tparams`trecovered`tilspy`tours`tdiff`n"
    foreach ($item in @($groups.$name.repositories)) {
        $safe = $item.repo.Replace('/', '__')
        $dir = Join-Path $srcRoot "$name\$safe"
        Write-Host ("==> {0}  {1}" -f $name, $item.repo) -ForegroundColor Cyan
        try {
            if ($Refresh -or -not (Test-Path -LiteralPath $dir)) {
                $zip = Join-Path $srcRoot "$safe.zip"
                $ref = if ($item.branch -eq 'HEAD') { 'HEAD' } else { "refs/heads/$($item.branch)" }
                Invoke-WebRequest -Uri "https://codeload.github.com/$($item.repo)/zip/$ref" -OutFile $zip -UseBasicParsing -UserAgent 'decompile-corpus'
                Expand-Repo $zip $dir
                Remove-Item -LiteralPath $zip -Force
            }
            $log = Join-Path $logRoot "$name-$safe.log"
            $outputs = @(Invoke-RepoBuild $dir $log)
            if ($outputs.Count -eq 0) {
                Add-Utf8Line $skippedPath ("$name`t$($item.repo)`t" + (Get-FirstError $log))
                Write-Host '    ビルドできないので除く' -ForegroundColor Yellow
                continue
            }
            $count = 0
            foreach ($output in $outputs) {
                try {
                    $outDir = Join-Path $work ("out\$name\$safe\$([IO.Path]::GetFileNameWithoutExtension($output.Name))")
                    if (Test-Path -LiteralPath $outDir) { Remove-Item -LiteralPath $outDir -Recurse -Force -ErrorAction SilentlyContinue }
                    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
                    $run = Invoke-Captured { & $Exe --decompile $output.Path $outDir }
                    $run.Lines | Out-Host
                    Add-Times $name $run.Lines
                    if ($run.Code -ne 0) {
                        Add-Utf8Line $skippedPath "$name`t$($item.repo)`t$($output.Name) を逆コンパイルできない"
                        continue
                    }
                    $run = Invoke-Captured { & $measure $output.Path $outDir $tsv $name $item.repo }
                    $run.Lines | Out-Host
                    Add-Times $name $run.Lines
                    if ($run.Code -ne 0) {
                        Add-Utf8Line $skippedPath "$name`t$($item.repo)`t$($output.Name) を ILSpy と比べられない"
                        continue
                    }
                    Add-Utf8Line $builtPath "$name`t$($item.repo)`t$($output.Name)`t$($output.Path)"
                    Remove-Item -LiteralPath $outDir -Recurse -Force -ErrorAction SilentlyContinue
                    $count++
                } catch {
                    Add-Utf8Line $skippedPath "$name`t$($item.repo)`t$($output.Name) $($_.Exception.Message.Replace("`t", ' ').Replace("`r", ' ').Replace("`n", ' '))"
                }
            }
            Write-Host "    アセンブリ $count"
        } catch {
            Add-Utf8Line $skippedPath "$name`t$($item.repo)`t$($_.Exception.Message.Replace("`t", ' ').Replace("`r", ' ').Replace("`n", ' '))"
            Write-Host "    除く: $($_.Exception.Message)" -ForegroundColor Yellow
        }
    }
}

function Format-Percent([int]$Numerator, [int]$Denominator) {
    if ($Denominator -eq 0) { return '-' }
    return [string]::Format([Globalization.CultureInfo]::InvariantCulture, '{0:0.0}%', ($Numerator * 100.0 / $Denominator))
}

$report = New-Object System.Collections.Generic.List[string]
$report.Add('逆コンパイルのコーパス（D-105）。Debug ビルド。ILSpy 8.2.0.7535。文字の一致は見ていない。')
$report.Add('戻せた = ILSpy にある async / イテレーター / foreach / using / lock / ラムダ / switch / ?. がこちらにもあり、IL のままでもなく、状態機械のままでもないもの。allow-conditional.txt にある行は、足りないのが ?. だけで戻しが using のときだけ、戻せたに数える。')
$report.Add('差分の内訳はメソッド数ではない（1 メソッドが複数の書き方でずれる）。差分割合はメソッド単位。こちらにだけある書き方は差分にしない。')
$report.Add('')

$builtRows = @(Import-Csv -LiteralPath $builtPath -Delimiter "`t" -Encoding UTF8)
$skipRows = @(Import-Csv -LiteralPath $skippedPath -Delimiter "`t" -Encoding UTF8)
$allow = @{}
$allowPath = Join-Path $root 'tests\decompile\allow-conditional.txt'
if (Test-Path -LiteralPath $allowPath) {
    foreach ($line in [IO.File]::ReadAllLines($allowPath)) {
        $trim = $line.Trim()
        if ($trim.Length -eq 0 -or $trim.StartsWith('#')) { continue }
        $allow[$trim] = $false
    }
}

function Test-ConditionalUsingAllowed([object]$Row) {
    $key = "$($Row.group)`t$($Row.repo)`t$($Row.assembly)`t$($Row.type)`t$($Row.method)`t$($Row.params)"
    if (-not $script:allow.ContainsKey($key)) { return $false }
    $ours = @($Row.ours -split ',')
    if ($ours -notcontains 'using') { return $false }
    $hard = @($Row.diff.Split(',') | Where-Object { $_ -and ($_ -like 'missing:*' -or $_ -eq 'il' -or $_ -eq 'statemachine' -or $_ -eq 'not-found') })
    if ($hard.Count -ne 1 -or $hard[0] -ne 'missing:conditional') { return $false }
    $script:allow[$key] = $true
    return $true
}

foreach ($name in $names) {
    $tsv = Join-Path $work "$name.tsv"
    $rows = @()
    if (Test-Path -LiteralPath $tsv) { $rows = @(Import-Csv -LiteralPath $tsv -Delimiter "`t" -Encoding UTF8) }
    $total = $rows.Count
    $ok = 0
    $allowed = 0
    foreach ($row in $rows) {
        if ($row.recovered -eq '1') { $ok++; continue }
        if (Test-ConditionalUsingAllowed $row) { $ok++; $allowed++ }
    }
    $diff = $total - $ok
    $repoCount = @($groups.$name.repositories).Count
    $reposBuilt = @($builtRows | Where-Object { $_.group -eq $name } | Select-Object -ExpandProperty repo -Unique).Count
    $reposSkipped = $repoCount - $reposBuilt
    $assemblies = @($builtRows | Where-Object { $_.group -eq $name }).Count
    $line = "{0,-8} ビルド {1}  除いた {2}  アセンブリ {3}  メソッド {4}  戻せた {5}  差分 {6}" -f $name, $reposBuilt, $reposSkipped, $assemblies, $total, (Format-Percent $ok $total), (Format-Percent $diff $total)
    $report.Add($line)
    Write-Host $line
    if ($allowed -gt 0) {
        $note = "         許可 $allowed （allow-conditional.txt。元のソースは using で ?. が無い。ILSpy が ?.Dispose() と数えた分）"
        $report.Add($note)
        Write-Host $note
    }
    $buckets = @{}
    foreach ($row in $rows) {
        if ($row.recovered -eq '1' -or $row.diff -eq '-' -or (Test-ConditionalUsingAllowed $row)) { continue }
        foreach ($part in $row.diff.Split(',')) {
            if ($part -and -not $part.StartsWith('extra:')) {
                if (-not $buckets.ContainsKey($part)) { $buckets[$part] = 0 }
                $buckets[$part]++
            }
        }
    }
    $ordered = @($buckets.GetEnumerator() | Sort-Object -Property Value -Descending)
    if ($ordered.Count -gt 0) {
        $bits = @($ordered | ForEach-Object { "{0} {1}" -f $_.Key, $_.Value })
        $report.Add('         ' + ($bits -join '  '))
        Write-Host ('         ' + ($bits -join '  '))
    }
    $extras = @{}
    foreach ($row in $rows) {
        if (-not $row.diff -or $row.diff -eq '-') { continue }
        foreach ($part in $row.diff.Split(',')) {
            if ($part.StartsWith('extra:')) {
                if (-not $extras.ContainsKey($part)) { $extras[$part] = 0 }
                $extras[$part]++
            }
        }
    }
    $extraOrdered = @($extras.GetEnumerator() | Sort-Object -Property Value -Descending)
    if ($extraOrdered.Count -gt 0) {
        $bits = @($extraOrdered | ForEach-Object { "{0} {1}" -f $_.Key, $_.Value })
        $report.Add('         こちらだけ  ' + ($bits -join '  '))
        Write-Host ('         こちらだけ  ' + ($bits -join '  '))
    }
    $engineMs = $speed[$name].Engine
    $ilspyMs = $speed[$name].Ilspy
    $ratio = if ($ilspyMs -gt 0) { $engineMs / $ilspyMs } else { 0 }
    $timing = [string]::Format($invariant, '         時間  エンジン {0:0.0} s  ILSpy {1:0.0} s  比 {2:0.00}', ($engineMs / 1000.0), ($ilspyMs / 1000.0), $ratio)
    $report.Add($timing)
    Write-Host $timing
}
$report.Add('')
$report.Add('除いたもの:')
if ($skipRows.Count -eq 0) { $report.Add('  (なし)') }
foreach ($skip in $skipRows) { $report.Add("  $($skip.group)  $($skip.repo)  $($skip.reason)") }

$stale = @($allow.GetEnumerator() | Where-Object { -not $_.Value -and ($names -contains $_.Key.Split("`t")[0]) } | ForEach-Object { $_.Key } | Sort-Object)
if ($stale.Count -gt 0) {
    $report.Add('')
    $report.Add("許可リストの消し忘れ $($stale.Count) （足りないのが ?. だけで、戻しが using、ではなくなった）")
    foreach ($key in $stale) { $report.Add("  $key") }
}
$reportPath = Join-Path $work 'report.txt'
Write-Utf8 $reportPath (($report -join "`r`n") + "`r`n") -Bom
Write-Host "wrote $reportPath"
if ($stale.Count -gt 0) {
    throw "許可リストに、もう当てはまらない行が $($stale.Count) あります。tests/decompile/allow-conditional.txt から消してください。"
}
